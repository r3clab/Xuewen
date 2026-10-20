//! Rendered page images for the file picker's preview pane.
//!
//! Always on, like annotations and projects: rendering needs no `[ai.*]`
//! configuration, so `App` holds this directly rather than behind an
//! `Option`. Unlike every other always-on service it takes no `SqlitePool` —
//! its inputs are a path and a content hash, both resolved by the handler, so
//! paper lookup and trash policy stay in one place (`App::paper`).

pub mod cache;
mod render;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use render::{PageMeta, RenderError};

/// How many pages may rasterize at once. hayro is synchronous CPU work whose
/// author is explicit that performance has had no attention yet, and a picker
/// scrubbed with the arrow keys can ask for pages far faster than they render
/// — this bounds what that costs the machine ingest, search and agent turns
/// are sharing. A small constant, not a core count: the k8s manifest requests
/// 100m CPU, where a host-derived bound would be meaningless.
const MAX_CONCURRENT_RENDERS: usize = 4;

pub(crate) struct PreviewService {
    cache_dir: PathBuf,
    meta: cache::MetaCache,
    renders: Arc<tokio::sync::Semaphore>,
}

impl PreviewService {
    /// Remembers a path; touches no disk. The directory appears on the first
    /// cache write, the way `FtsIndex::open` creates its own index dir.
    pub(crate) fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            meta: cache::MetaCache::default(),
            renders: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_RENDERS)),
        }
    }

    /// Page count and first-page geometry. Parses the PDF but rasterizes
    /// nothing, and the answer is remembered for the service's lifetime.
    pub(crate) async fn meta(
        &self,
        pdf_path: &Path,
        content_hash: &str,
    ) -> Result<PageMeta, PreviewError> {
        if let Some(m) = self.meta.get(content_hash) {
            return Ok(m);
        }
        let bytes = read_pdf(pdf_path).await?;
        let meta = self.blocking(move || render::page_meta(bytes)).await?;
        self.meta.put(content_hash, meta);
        Ok(meta)
    }

    /// One page as PNG bytes, rendering it if the cache doesn't have it.
    pub(crate) async fn page_png(
        &self,
        pdf_path: &Path,
        content_hash: &str,
        page: u32,
    ) -> Result<Vec<u8>, PreviewError> {
        if let Some(png) = cache::read(&self.cache_dir, content_hash, page).await {
            return Ok(png);
        }
        let bytes = read_pdf(pdf_path).await?;
        let png = self.blocking(move || render::page_png(bytes, page)).await?;
        // A failed write costs a re-render next time, nothing more, so it
        // must not fail the request that already has the bytes in hand.
        if let Err(e) = cache::write(&self.cache_dir, content_hash, page, &png).await {
            tracing::warn!("preview cache write failed: {e}");
        }
        Ok(png)
    }

    pub(crate) async fn forget(&self, content_hash: &str) {
        let _ = tokio::fs::remove_dir_all(cache::paper_dir(&self.cache_dir, content_hash)).await;
    }

    /// Run one rasterization off the async executor, under the concurrency
    /// bound. A panic inside hayro surfaces as a `JoinError` here rather than
    /// taking the worker down, so it degrades to the same text-card fallback
    /// an encrypted PDF gets.
    async fn blocking<T, F>(&self, f: F) -> Result<T, PreviewError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, RenderError> + Send + 'static,
    {
        let _permit = self
            .renders
            .acquire()
            .await
            .map_err(|_| PreviewError::Unrenderable)?;
        match tokio::task::spawn_blocking(f).await {
            Ok(r) => r.map_err(PreviewError::from),
            Err(e) => {
                tracing::error!("preview render panicked: {e}");
                Err(PreviewError::Unrenderable)
            }
        }
    }
}

/// Read the library file. A missing or unreadable file is `Unrenderable`
/// rather than an internal error: from the picker's side it is the same
/// answer — this paper has no image to show — and the text card covers it.
async fn read_pdf(path: &Path) -> Result<Vec<u8>, PreviewError> {
    tokio::fs::read(path)
        .await
        .map_err(|_| PreviewError::Unrenderable)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PreviewError {
    /// Nothing can be shown for this paper. The one signal the frontend needs
    /// to fall back to the text card, so it answers 422 rather than 500.
    Unrenderable,
    PageOutOfRange,
}

impl From<RenderError> for PreviewError {
    fn from(e: RenderError) -> Self {
        match e {
            RenderError::Unrenderable => PreviewError::Unrenderable,
            RenderError::PageOutOfRange => PreviewError::PageOutOfRange,
        }
    }
}

impl std::fmt::Display for PreviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PreviewError::Unrenderable => f.write_str("the PDF could not be rendered"),
            PreviewError::PageOutOfRange => f.write_str("page out of range"),
        }
    }
}

impl std::error::Error for PreviewError {}
