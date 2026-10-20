pub mod fetch;
mod fields;
pub(crate) mod import;
pub mod pipeline;
pub mod refresh;
mod source;
pub mod watcher;

use std::path::Path;

use fetch::Fetcher;
use import::{ImportError, Imported};
use pipeline::IngestCtx;

/// Held by `App` rather than cloned because `IngestCtx` is not `Clone` (its
/// `Resolver`/`Grobid` are not).
pub struct Ingest {
    ctx: IngestCtx,
    /// HTTP fetcher for URL imports, built once: its two `reqwest::Client`s
    /// carry connection pools that per-request construction would discard.
    /// Also owns the EZproxy login prefix (`[proxy].login_url`).
    fetcher: Fetcher,
}

impl Ingest {
    pub fn new(ctx: IngestCtx, fetcher: Fetcher) -> Self {
        Self { ctx, fetcher }
    }

    pub fn staging_dir(&self) -> &Path {
        &self.ctx.dirs.staging_dir
    }

    pub(crate) fn ctx(&self) -> &IngestCtx {
        &self.ctx
    }

    pub(crate) fn proxy_host(&self) -> Option<&str> {
        self.fetcher.proxy_host()
    }

    pub(crate) async fn import(
        &self,
        input: &str,
        cookie: Option<&str>,
    ) -> Result<Imported, ImportError> {
        import::fetch_stage_ingest(&self.ctx, &self.fetcher, input, cookie).await
    }
}
