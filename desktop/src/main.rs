mod bootstrap;
mod bundle;

use anyhow::{Context, Result};
use tauri::Manager;

fn main() {
    if let Err(e) = run() {
        show_fatal(&e);
        std::process::exit(1);
    }
}

/// Blocking error dialog — the only channel a GUI app has to a user whose
/// terminal-less launch just failed.
fn show_fatal(err: &anyhow::Error) {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("Xuewen failed to start")
        .set_description(format!("{err:#}"))
        .show();
}

fn run() -> Result<()> {
    let dirs = bootstrap::AppDirs::resolve()?;
    let cfg_path = bootstrap::ensure_bootstrap(&dirs)?;

    // Environment first, before any thread exists: the `env` file (API keys),
    // then PATH so the bundled pdftotext/node sidecars shadow system copies —
    // src/pdf.rs and src/agent resolve both via `Command::new("...")`.
    if let Ok(text) = std::fs::read_to_string(dirs.env_file()) {
        for (k, v) in bootstrap::parse_env_file(&text) {
            std::env::set_var(k, v);
        }
    }
    let bundle = bundle::bundle_dirs();
    if let Some((macos_dir, _)) = &bundle {
        let path = std::env::var("PATH").ok();
        std::env::set_var("PATH", bootstrap::prepend_path(macos_dir, path.as_deref()));
    }

    // A GUI app has no terminal: log to ~/Library/Logs/Xuewen/.
    let file = tracing_appender::rolling::never(&dirs.logs, "xuewen-desktop.log");
    let (writer, log_guard) = tracing_appender::non_blocking(file);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(writer)
        .with_ansi(false)
        .init();
    // The guard flushes and joins the log worker on drop, and process::exit
    // runs no destructors — so the setup failure path below must drop it by
    // hand or its one "startup failed" line can be lost. Shared, not moved:
    // the setup closure is FnOnce and drops its captures when it returns, so
    // moving the guard in would kill logging for the app's whole life on the
    // success path too. Tauri's run loop also ends in process::exit, so the
    // exit handler drops it after the backend's shutdown lines.
    let log_guard = std::sync::Arc::new(std::sync::Mutex::new(Some(log_guard)));

    let mut cfg = xuewen::config::Config::load(&cfg_path)
        .with_context(|| format!("config file: {}", cfg_path.display()))?;
    if let Some((_, resources)) = &bundle {
        if cfg.ai.agent.runner.is_none() {
            // In-memory only — never written back to the user's config.
            cfg.ai.agent.runner = Some(resources.join("agent-runner/src/runner.mjs"));
        }
    }

    // The backend runs on our own tokio runtime; hand its handle to Tauri so
    // there is exactly one runtime in the process. `rt` stays in scope for
    // the app's whole life.
    let rt = tokio::runtime::Runtime::new()?;
    tauri::async_runtime::set(rt.handle().clone());

    // Backend bring-up happens inside `.setup()`, which Tauri runs *after*
    // plugin initialization: a second app launch is terminated by the
    // single-instance plugin before it ever touches the SQLite db, spawns
    // services, or binds a port.
    let setup_cfg_path = cfg_path.clone();
    let setup_log_guard = log_guard.clone();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // Second launch: focus the existing window instead of racing the
            // first instance for the SQLite db and schedulers.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(move |app| {
            // Tauri turns an `Err` returned from setup into a panic whose
            // message goes only to stderr — invisible for a GUI app — so
            // failures here show the error dialog directly (setup runs on
            // the main thread, where rfd is safe) and exit, instead of
            // propagating through `.run()`'s Result.
            if let Err(e) = start_backend(app, &cfg).with_context(|| {
                format!(
                    "while starting backend (config: {})",
                    setup_cfg_path.display()
                )
            }) {
                tracing::error!("startup failed: {e:#}");
                // Flush before exiting, or the line above dies in the
                // non-blocking writer's channel.
                drop(setup_log_guard.lock().unwrap().take());
                show_fatal(&e);
                std::process::exit(1);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .with_context(|| {
            format!(
                "running tauri application (config file: {})",
                cfg_path.display()
            )
        })?;
    app.run(move |handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(backend) = handle.try_state::<xuewen::app::App>() {
                tauri::async_runtime::block_on(backend.shutdown());
            }
            drop(log_guard.lock().unwrap().take());
        }
    });
    Ok(())
}

/// Backend bring-up + webview creation, called from `.setup()`: connect the
/// db, spawn services, bind an ephemeral loopback port, spawn the server
/// future, and open the window pointed at it.
fn start_backend(app: &tauri::App, cfg: &xuewen::config::Config) -> Result<()> {
    let backend = tauri::async_runtime::block_on(async {
        let pool = xuewen::db::connect(&cfg.database_url).await?;
        xuewen::app::App::start(cfg, pool).await
    })?;
    if let Err(e) = open_window(app, &backend) {
        tauri::async_runtime::block_on(backend.shutdown());
        return Err(e);
    }
    app.manage(backend);
    Ok(())
}

fn open_window(app: &tauri::App, backend: &xuewen::app::App) -> Result<()> {
    let addr = tauri::async_runtime::block_on(async {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let server = xuewen::web::serve_on(listener, backend.clone(), backend.tasks().token());
        tokio::spawn(async move {
            if let Err(e) = server.await {
                tracing::error!("server exited: {e:#}");
            }
        });
        anyhow::Ok(addr)
    })?;
    tracing::info!("desktop backend on http://{addr}");

    let url: tauri::Url = format!("http://{addr}")
        .parse()
        .context("building backend url")?;
    tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::External(url))
        .title("Xuewen")
        .inner_size(1280.0, 800.0)
        .build()?;
    Ok(())
}
