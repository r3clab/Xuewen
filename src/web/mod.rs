mod assets;
mod error;
mod handlers;

use std::future::Future;

use anyhow::Result;
use axum::Router;
use tokio_util::sync::CancellationToken;

use crate::app::App;

pub fn router(app: App) -> Router {
    handlers::router().with_state(app)
}

/// Serve the full router on a listener the caller has already bound —
/// lets the caller bind port 0 and learn the real port from
/// `listener.local_addr()` before starting the server. Shuts down
/// gracefully once `shutdown` is cancelled. Non-async on purpose: the
/// returned future is `'static` and can be `tokio::spawn`ed.
pub fn serve_on(
    listener: tokio::net::TcpListener,
    app: App,
    shutdown: CancellationToken,
) -> impl Future<Output = Result<()>> + Send + 'static {
    let router = router(app);
    async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await?;
        Ok(())
    }
}
