use anyhow::Result;
use tokio_util::sync::CancellationToken;

use super::Ctx;
use crate::app::App;

#[derive(clap::Args)]
pub(super) struct ServeArgs {
    /// Address to bind.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// Port to bind.
    #[arg(long, default_value_t = 8080)]
    port: u16,
    /// Allow binding a non-loopback address (mutating endpoints have no auth).
    #[arg(long)]
    allow_remote: bool,
}

/// Cancel `token` on SIGINT or SIGTERM. The container runs the server as
/// PID 1, which gets no default signal disposition — without an explicit
/// handler `docker stop` and Kubernetes rollouts hang the full grace period
/// and end in SIGKILL.
pub(super) async fn cancel_on_signal(token: CancellationToken) {
    let mut signals = Signals::install();
    signals.recv().await;
    token.cancel();
    std::process::exit(signals.recv().await);
}

#[cfg(unix)]
struct Signals {
    interrupt: tokio::signal::unix::Signal,
    terminate: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl Signals {
    fn install() -> Self {
        use tokio::signal::unix::{signal, SignalKind};
        Self {
            interrupt: signal(SignalKind::interrupt())
                .expect("failed to install the Ctrl-C handler"),
            terminate: signal(SignalKind::terminate())
                .expect("failed to install the SIGTERM handler"),
        }
    }

    async fn recv(&mut self) -> i32 {
        tokio::select! {
            _ = self.interrupt.recv() => 128 + libc::SIGINT,
            _ = self.terminate.recv() => 128 + libc::SIGTERM,
        }
    }
}

#[cfg(not(unix))]
struct Signals;

#[cfg(not(unix))]
impl Signals {
    fn install() -> Self {
        Self
    }

    async fn recv(&mut self) -> i32 {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install the Ctrl-C handler");
        130
    }
}

pub(super) async fn serve(ctx: &Ctx, args: ServeArgs) -> Result<()> {
    let ServeArgs {
        host,
        port,
        allow_remote,
    } = args;
    let (cfg, pool) = (&ctx.cfg, &ctx.pool);
    if !is_loopback_host(&host) {
        if allow_remote {
            eprintln!(
                "warning: binding {host}: the web UI has mutating endpoints and no auth — \
                 anyone who can reach this address can import and delete papers"
            );
        } else {
            anyhow::bail!(
                "refusing to bind non-loopback address {host}: the web UI has no auth; \
                 pass --allow-remote to override"
            );
        }
    }
    let app = App::start(cfg, pool.clone()).await?;
    let served = listen(&app, &host, port).await;
    app.shutdown().await;
    served
}

async fn listen(app: &App, host: &str, port: u16) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}")).await?;
    tracing::info!("xuewen serving on http://{host}:{port}");
    tokio::spawn(cancel_on_signal(app.tasks().token()));
    crate::web::serve_on(listener, app.clone(), app.tasks().token()).await
}

/// Whether `host` is a loopback bind (safe to serve without auth). Non-IP
/// hostnames other than "localhost" are conservatively treated as remote.
fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::is_loopback_host;

    #[test]
    fn classifies_loopback_hosts() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("127.1.2.3"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("LOCALHOST"));
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("192.168.1.10"));
        assert!(!is_loopback_host("example.com"));
    }
}
