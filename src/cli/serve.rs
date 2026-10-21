use anyhow::Result;

use super::signals::cancel_on_signal;
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
