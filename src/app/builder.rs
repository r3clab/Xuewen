use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use sqlx::SqlitePool;

use super::lazy::Lazy;
use super::{summarize, App, Inner};
use crate::agent::AgentService;
use crate::annotations::AnnotationsService;
use crate::citations::CitationsService;
use crate::config::{AgentConfig, Config, UiConfig};
use crate::daily::{self, DailyService};
use crate::http::RetryPolicy;
use crate::ingest::fetch::Fetcher;
use crate::ingest::pipeline::IngestCtx;
use crate::ingest::Ingest;
use crate::preview::PreviewService;
use crate::search::{indexer, SearchService};
use crate::summary::SummaryService;
use crate::tasks::Tasks;
use crate::translate::TranslateService;

/// Base state with every optional service off (heuristics-only
/// citations, no ingest/search/daily/agent, default UI prefs). Tests
/// compose on top of this — set just the services they need, then
/// `build` — so no per-service constructor exists or needs adding when
/// the next service lands.
pub struct AppBuilder {
    inner: Inner,
}

impl AppBuilder {
    pub(super) fn new(pool: SqlitePool, library_root: PathBuf) -> Self {
        let citations = CitationsService::heuristic_only(pool.clone());
        let annotations = Arc::new(AnnotationsService::new(pool.clone()));
        // Under the library root so a read-only or test router needs no
        // config: `serve` overrides it from `[preview]`.
        let preview = Arc::new(PreviewService::new(library_root.join("preview-cache")));
        let agent = AgentConfig::default();
        Self {
            inner: Inner {
                pool,
                library_root,
                ingest: None,
                search: None,
                daily: None,
                agent: None,
                citations,
                annotations,
                translate: None,
                preview,
                ui: UiConfig::default(),
                summary: Lazy::deferred(|| Err(summarize::NOT_CONFIGURED.to_string())),
                clone_allowed_hosts: agent.clone_allowed_hosts,
                max_repo_mb: agent.max_repo_mb,
                tasks: Tasks::new(),
            },
        }
    }

    pub fn ingest(mut self, ingest: Ingest) -> Self {
        self.inner.ingest = Some(Lazy::Ready(ingest));
        self
    }

    pub(super) fn lazy_ingest(
        mut self,
        make: impl Fn() -> Result<Ingest> + Send + Sync + 'static,
    ) -> Self {
        self.inner.ingest = Some(Lazy::deferred(make));
        self
    }

    pub fn search(mut self, search: impl Into<Option<Arc<SearchService>>>) -> Self {
        self.inner.search = search.into();
        self
    }

    pub fn daily(mut self, daily: impl Into<Option<Arc<DailyService>>>) -> Self {
        self.inner.daily = daily.into();
        self
    }

    pub fn agent(mut self, agent: impl Into<Option<Arc<AgentService>>>) -> Self {
        self.inner.agent = agent.into();
        self
    }

    pub fn citations(mut self, citations: Arc<CitationsService>) -> Self {
        self.inner.citations = citations;
        self
    }

    pub fn translate(mut self, translate: impl Into<Option<Arc<TranslateService>>>) -> Self {
        self.inner.translate = translate.into();
        self
    }

    pub(super) fn preview(mut self, preview: Arc<PreviewService>) -> Self {
        self.inner.preview = preview;
        self
    }

    pub(super) fn ui(mut self, ui: UiConfig) -> Self {
        self.inner.ui = ui;
        self
    }

    pub(super) fn summary(mut self, summary: Lazy<Arc<SummaryService>, String>) -> Self {
        self.inner.summary = summary;
        self
    }

    pub fn clone_allowed_hosts(mut self, hosts: Vec<String>) -> Self {
        self.inner.clone_allowed_hosts = hosts;
        self
    }

    pub(super) fn max_repo_mb(mut self, max_repo_mb: u64) -> Self {
        self.inner.max_repo_mb = max_repo_mb;
        self
    }

    pub fn build(self) -> App {
        App {
            inner: Arc::new(self.inner),
        }
    }
}

fn from_config(cfg: &Config, pool: SqlitePool) -> AppBuilder {
    App::builder(pool, cfg.library_root.clone())
        // No config to gate, only a place to cache: previews are always on.
        .preview(Arc::new(PreviewService::new(cfg.preview.cache_dir.clone())))
        .ui(cfg.ui.clone())
        .clone_allowed_hosts(cfg.ai.agent.clone_allowed_hosts.clone())
        .max_repo_mb(cfg.ai.agent.max_repo_mb)
}

/// Build all services and spawn their background loops (indexer, daily
/// scheduler, summary worker). Interactive retry policy: uploads answer
/// synchronously, so keep resolver retries short.
pub(super) async fn server(cfg: &Config, pool: SqlitePool) -> Result<App> {
    let ingest = ingest(cfg, pool.clone(), RetryPolicy::interactive())?;
    let search = match SearchService::open(pool.clone(), &cfg.search, &cfg.ai).await {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::warn!("search disabled: {e}");
            None
        }
    };
    let daily = DailyService::from_config(cfg, pool.clone())?;
    let summary = SummaryService::from_config(pool.clone(), cfg);
    let agent = AgentService::from_config(&cfg.ai.agent);
    match &agent {
        None => tracing::info!("agent ask disabled (no [ai.agent] backends)"),
        Some(a) => {
            for p in a.preflight().await {
                tracing::warn!("agent ask: {p}");
            }
        }
    }
    // Unconditional (rows and staging dirs can predate an [ai.agent] removal):
    // resolve 'cloning' rows a crash stranded and sweep orphaned staging dirs.
    crate::agent::code::reconcile_startup(&pool, &cfg.library_root).await;

    // Workers start only after the last fallible step, so a failed start
    // leaves nothing running.
    let builder = from_config(cfg, pool.clone());
    let tasks = &builder.inner.tasks;
    if let Some(s) = &search {
        indexer::spawn(tasks, s.clone(), cfg.library_root.clone());
    }
    if let Some(d) = &daily {
        tasks.spawn(daily::scheduler::run(d.clone(), tasks.token()));
    }
    if let Some(s) = &summary {
        tasks.spawn(crate::summary::run(
            s.clone(),
            std::time::Duration::from_secs(60),
            tasks.token(),
        ));
    }
    let summary = match summary {
        Some(s) => Lazy::Ready(s),
        None => {
            let reason = summarize::disabled_reason(cfg);
            Lazy::deferred(move || Err(reason.clone()))
        }
    };
    Ok(builder
        .ingest(ingest)
        .search(search)
        .daily(daily)
        .summary(summary)
        .agent(agent)
        .citations(CitationsService::from_config(pool, cfg))
        .translate(TranslateService::from_config(cfg).map(Arc::new))
        .build())
}

pub(super) fn cli(cfg: &Config, pool: SqlitePool) -> App {
    let shared = Arc::new(cfg.clone());
    // Only the commands that ingest build an IngestCtx (two HTTP clients),
    // always with the production retry policy; `serve` gets its own
    // interactive one inside `App::start`.
    let make_ingest = {
        let (cfg, pool) = (shared.clone(), pool.clone());
        move || ingest(&cfg, pool.clone(), RetryPolicy::production())
    };
    let summary = {
        let (cfg, pool) = (shared, pool.clone());
        move || {
            SummaryService::from_config(pool.clone(), &cfg)
                .ok_or_else(|| summarize::disabled_reason(&cfg))
        }
    };
    from_config(cfg, pool)
        .lazy_ingest(make_ingest)
        .summary(Lazy::deferred(summary))
        .agent(AgentService::from_config(&cfg.ai.agent))
        .build()
}

fn ingest(cfg: &Config, pool: SqlitePool, retry: RetryPolicy) -> Result<Ingest> {
    let ctx = IngestCtx::from_config(cfg, pool, retry)?;
    let fetcher = Fetcher::new(cfg.proxy.as_ref().map(|p| p.login_url.clone()))?;
    Ok(Ingest::new(ctx, fetcher))
}
