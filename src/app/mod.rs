mod annotate;
mod ask;
mod builder;
pub(crate) mod code;
pub(crate) mod error;
mod identify;
mod import;
mod lazy;
mod purge;
pub(crate) mod settings;
pub(crate) mod summarize;
mod translate;
mod trash;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use sqlx::SqlitePool;

use crate::agent::AgentService;
use crate::annotations::AnnotationsService;
use crate::citations::CitationsService;
use crate::config::{Config, UiConfig};
use crate::daily::DailyService;
use crate::ingest::Ingest;
use crate::library::model::Paper;
use crate::preview::PreviewService;
use crate::search::vector::VectorStore;
use crate::search::SearchService;
use crate::summary::SummaryService;
use crate::tasks::Tasks;
use crate::translate::TranslateService;
use builder::AppBuilder;
use error::Error;
use lazy::Lazy;

const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct App {
    inner: Arc<Inner>,
}

struct Inner {
    pool: SqlitePool,
    library_root: PathBuf,
    /// Present only when the server was started to allow uploads (`serve`),
    /// or once a CLI command first asks for it.
    ingest: Option<Lazy<Ingest>>,
    /// Present when a search index/service was opened (serve). `None` in
    /// read-only test routers -> /api/search answers 503.
    search: Option<Arc<SearchService>>,
    /// Present when `serve` holds the vector store: its address is published
    /// through it, and it is closed at shutdown so the lock goes even if a
    /// task outlives the grace period.
    vectors: Option<VectorStore>,
    /// Present when daily arXiv recommendations are configured (`serve`).
    /// `None` -> /api/daily answers 503.
    daily: Option<Arc<DailyService>>,
    /// Present when Agent Ask is configured (`serve` and the CLI). `None` ->
    /// chat endpoints answer 503 / available:false.
    agent: Option<Arc<AgentService>>,
    /// Always present: heuristic parsing needs no config; [ai.citations]
    /// adds the LLM fallback for entries heuristics can't parse.
    citations: Arc<CitationsService>,
    /// Always present: reader annotations are plain storage with no `[ai.*]`
    /// gate, so unlike the LLM-backed services this is never `Option`al.
    annotations: Arc<AnnotationsService>,
    /// Built at startup by `serve`, on first use by the CLI. Unconfigured
    /// -> /api/translate answers 503, /api/settings reports disabled.
    translate: Lazy<Arc<TranslateService>, String>,
    /// Always present: rasterizing a page needs no `[ai.*]` gate either, and
    /// unlike the other always-on services it holds no pool — only where its
    /// renders are cached.
    preview: Arc<PreviewService>,
    /// UI-facing preferences (e.g. abstract folding). Defaulted in
    /// read-only/test routers; set from config in `serve`.
    ui: UiConfig,
    summary: Lazy<Arc<SummaryService>, String>,
    /// Extra repo-clone hosts permitted beyond the built-in forges.
    clone_allowed_hosts: Vec<String>,
    max_repo_mb: u64,
    tasks: Tasks,
    /// Set when a mutation would have woken the indexer but this process
    /// runs none (the CLI), so the caller can catch the index up itself.
    index_dirty: AtomicBool,
}

/// Whether an operation accepts a trashed paper. Named at every
/// `fetch_paper` and `App::paper` call site so each one's trash policy is a
/// decision, not an accident of which query it happened to use.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Trash {
    /// Trashed papers 404: the UI no longer shows the paper (chat, code).
    Deny,
    /// Trashed papers still resolve: an open reader tab on a just-trashed
    /// paper keeps working, and annotations must survive a restore.
    Allow,
}

impl App {
    pub async fn start(cfg: &Config, pool: SqlitePool) -> Result<App> {
        builder::server(cfg, pool).await
    }

    pub(crate) fn for_cli(cfg: &Config, pool: SqlitePool) -> App {
        builder::cli(cfg, pool)
    }

    pub fn builder(pool: SqlitePool, library_root: PathBuf) -> AppBuilder {
        AppBuilder::new(pool, library_root)
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.inner.pool
    }

    pub(crate) fn library_root(&self) -> &Path {
        &self.inner.library_root
    }

    pub fn tasks(&self) -> &Tasks {
        &self.inner.tasks
    }

    pub async fn shutdown(&self) {
        self.inner.tasks.shutdown(SHUTDOWN_GRACE).await;
        if let Some(search) = &self.inner.search {
            search.release_writer();
        }
        if let Some(vectors) = &self.inner.vectors {
            vectors.close_gracefully().await;
        }
        // With the pool closed there is nothing left to catch the index up
        // from; whoever ran the indexer loop owned freshness until now.
        self.inner.index_dirty.store(false, Ordering::SeqCst);
        self.inner.pool.close().await;
    }

    pub(crate) async fn publish_address(&self, addr: SocketAddr) {
        if let Some(vectors) = &self.inner.vectors {
            if let Err(e) = vectors.publish_addr(addr).await {
                tracing::warn!("could not publish the server address for the CLI: {e:#}");
            }
        }
    }

    pub(crate) fn ui(&self) -> &UiConfig {
        &self.inner.ui
    }

    pub fn ingest(&self) -> Result<&Ingest, Error> {
        let ingest = self
            .inner
            .ingest
            .as_ref()
            .ok_or_else(|| Error::Unavailable("import not configured".into()))?;
        Ok(ingest.get()?)
    }

    pub(crate) fn search(&self) -> Result<&Arc<SearchService>, Error> {
        self.inner
            .search
            .as_ref()
            .ok_or_else(|| Error::Unavailable("search not configured".into()))
    }

    pub fn daily(&self) -> Result<&Arc<DailyService>, Error> {
        self.inner
            .daily
            .as_ref()
            .ok_or_else(|| Error::Unavailable("daily papers not configured".into()))
    }

    pub fn agent(&self) -> Result<&Arc<AgentService>, Error> {
        self.inner.agent.as_ref().ok_or_else(|| {
            Error::Unavailable(
                "agent ask is not configured — set [ai.agent.claude_code] or [ai.agent.codex]"
                    .into(),
            )
        })
    }

    pub fn translate(&self) -> Result<&Arc<TranslateService>, Error> {
        self.inner.translate.get().map_err(Error::Unavailable)
    }

    pub(crate) fn citations(&self) -> &CitationsService {
        &self.inner.citations
    }

    pub(crate) fn annotations(&self) -> &AnnotationsService {
        &self.inner.annotations
    }

    pub(crate) fn preview(&self) -> &PreviewService {
        &self.inner.preview
    }

    pub(crate) fn wake_search(&self) {
        match &self.inner.search {
            Some(s) => s.wake(),
            None => self.inner.index_dirty.store(true, Ordering::SeqCst),
        }
    }

    /// Whether a mutation left the search index stale since the last call.
    pub(crate) fn take_index_dirty(&self) -> bool {
        self.inner.index_dirty.swap(false, Ordering::SeqCst)
    }

    /// Look up a paper and apply the caller's trash policy. Doing this before a
    /// write makes a bad id a clean 404 rather than an FK-violation 500.
    pub(crate) async fn paper(&self, id: &str, trash: Trash) -> Result<Paper, Error> {
        match crate::library::papers::get(self.pool(), id).await? {
            Some(p) if trash == Trash::Allow || p.deleted_at.is_none() => Ok(p),
            _ => Err(Error::not_found()),
        }
    }
}
