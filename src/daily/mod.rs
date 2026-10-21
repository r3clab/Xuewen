mod feed;
mod job;
pub(crate) mod scheduler;
mod score;
pub mod store;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use sqlx::SqlitePool;

use crate::config::{Config, DailyConfig};
use crate::http::{user_agent, HttpClient, RetryPolicy};
use crate::llm::LlmClient;
use crate::search::embedder::Embedder;
use crate::search::vector::VectorStore;
use crate::search::Semantic;
use crate::tasks::Tasks;

const ARXIV_FEED_BASE: &str = "https://rss.arxiv.org/atom";
const ARXIV_PDF_BASE: &str = "https://arxiv.org/pdf";
const ARXIV_ABS_BASE: &str = "https://arxiv.org/abs";

/// Daily arXiv recommendations. Owns its own HTTP clients (all stateless);
/// only the vector store is shared with `SearchService`, since one process
/// can hold it at a time.
pub struct DailyService {
    cfg: DailyConfig,
    pool: SqlitePool,
    /// Feed fetches: retried like the resolvers.
    pub(crate) http: HttpClient,
    /// PDF downloads (bytes; single attempt — the TL;DR chain absorbs failures).
    pub(crate) plain_http: reqwest::Client,
    pub(crate) embedder: Embedder,
    pub(crate) vectors: VectorStore,
    pub(crate) chat: LlmClient,
    pub(crate) feed_base: String,
    pub(crate) pdf_base: String,
    running: AtomicBool,
}

/// Why daily recommendations are off, worded for whoever has to fix the
/// config.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Disabled(pub(crate) String);

impl DailyService {
    /// `Ok(None)` when the feature is off (warned, unless `[daily]` is simply
    /// absent). `Err` only on invalid `[daily]` values.
    pub(crate) fn from_config(
        cfg: &Config,
        pool: SqlitePool,
        semantic: &Semantic,
    ) -> anyhow::Result<Option<Arc<Self>>> {
        Ok(match Self::try_from_config(cfg, pool, semantic)? {
            Ok(svc) => Some(svc),
            Err(Disabled(reason)) => {
                if cfg.daily.is_some() {
                    tracing::warn!("{reason} — daily papers disabled");
                }
                None
            }
        })
    }

    /// Like `from_config`, but says why the feature is off: no `[daily]`
    /// section, no `[ai.embedding]`/`[ai.daily]`, a missing model or API key,
    /// or no vector store.
    pub(crate) fn try_from_config(
        cfg: &Config,
        pool: SqlitePool,
        semantic: &Semantic,
    ) -> anyhow::Result<Result<Arc<Self>, Disabled>> {
        let off = |reason: String| Ok(Err(Disabled(reason)));
        let Some(daily) = &cfg.daily else {
            return off("[daily] is not configured".into());
        };
        if daily.categories.is_empty() {
            anyhow::bail!("[daily].categories must not be empty");
        }
        scheduler::parse_run_at(&daily.run_at)?; // fail fast on typos
        let Some(embed) = &cfg.ai.embedding else {
            return off("[daily] is set but [ai.embedding] is missing".into());
        };
        let er = cfg.ai.resolve(&embed.endpoint);
        let emodel = embed.model();
        let Some(embedder) = Embedder::from_resolved(&er, &emodel, embed.dims) else {
            return off(er.missing_key_reason("ai.embedding"));
        };
        let Some(daily_use) = &cfg.ai.daily else {
            return off("[daily] is set but [ai.daily] is missing".into());
        };
        let dr = cfg.ai.resolve(daily_use);
        let Some(chat) = LlmClient::from_resolved(&dr) else {
            return off(dr.unusable_reason("ai.daily"));
        };
        let Some(vectors) = semantic.store().cloned() else {
            let reason = semantic.reason().unwrap_or_default();
            return off(format!("the vector store is unavailable ({reason})"));
        };
        let ua = user_agent(cfg.contact_email.as_deref());
        Ok(Ok(Arc::new(Self {
            cfg: daily.clone(),
            pool,
            http: HttpClient::new(
                reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .user_agent(&ua)
                    .build()?,
                RetryPolicy::production(),
            ),
            plain_http: reqwest::Client::builder().user_agent(&ua).build()?,
            embedder,
            vectors,
            chat,
            feed_base: ARXIV_FEED_BASE.to_string(),
            pdf_base: ARXIV_PDF_BASE.to_string(),
            running: AtomicBool::new(false),
        })))
    }

    /// DI constructor: every remote endpoint is overridable. Test support only.
    pub fn for_tests(
        cfg: DailyConfig,
        pool: SqlitePool,
        embedder: Embedder,
        vectors: VectorStore,
        chat: LlmClient,
        feed_base: &str,
        pdf_base: &str,
    ) -> Arc<Self> {
        let ua = user_agent(None);
        Arc::new(Self {
            cfg,
            pool,
            http: HttpClient::new(
                reqwest::Client::builder().user_agent(&ua).build().unwrap(),
                RetryPolicy::fast_for_tests(),
            ),
            plain_http: reqwest::Client::builder().user_agent(&ua).build().unwrap(),
            embedder,
            vectors,
            chat,
            feed_base: feed_base.trim_end_matches('/').to_string(),
            pdf_base: pdf_base.trim_end_matches('/').to_string(),
            running: AtomicBool::new(false),
        })
    }

    #[cfg(test)]
    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn try_begin(&self) -> bool {
        self.running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Run for `batch_date` unless a run is already in flight (then `None`).
    pub(crate) async fn run_guarded(&self, batch_date: &str) -> Option<store::DailyRun> {
        if !self.try_begin() {
            return None;
        }
        let _guard = RunFlagGuard(&self.running);
        Some(job::run_once(self, batch_date).await)
    }

    /// Guarded run on a background task; `false` if one was in flight.
    /// The guard is taken synchronously, so a caller seeing `true` knows
    /// the very next `spawn_run`/`run_guarded` will refuse.
    pub(crate) fn spawn_run(self: &Arc<Self>, batch_date: String, tasks: &Tasks) -> bool {
        if !self.try_begin() {
            return false;
        }
        let svc = self.clone();
        tasks.spawn(async move {
            let _guard = RunFlagGuard(&svc.running);
            let run = job::run_once(&svc, &batch_date).await;
            tracing::info!("daily run {run}");
        });
        true
    }
}

/// Clears the running flag on scope exit — including panics and
/// cancellation — so a wedged run can never permanently refuse new runs.
struct RunFlagGuard<'a>(&'a AtomicBool);

impl Drop for RunFlagGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{temp_pool, temp_vectors};

    const BASE: &str = r#"
inbox_dir = "/i"
library_root = "/l"
database_url = "sqlite::memory:"
"#;
    const DAILY: &str = "[daily]\ncategories = [\"cs.AI\"]\n";
    const EMBEDDING: &str = "[ai.embedding]\napi_key = \"k\"\ndims = 4\n";
    const UNSET_KEY: &str = "api_key_env = \"XUEWEN_TEST_UNSET_KEY\"\n";

    async fn reason(sections: &[&str], semantic: Semantic) -> String {
        let cfg: Config = toml::from_str(&format!("{BASE}{}", sections.concat())).unwrap();
        let (_dir, pool) = temp_pool().await;
        match DailyService::try_from_config(&cfg, pool, &semantic).unwrap() {
            Ok(_) => "enabled".into(),
            Err(Disabled(reason)) => reason,
        }
    }

    async fn ready() -> Semantic {
        Semantic::Ready(crate::search::SemanticTier {
            embedder: Embedder::for_tests("http://unused", "m1", 4),
            store: temp_vectors(4).await,
        })
    }

    #[tokio::test]
    async fn each_missing_piece_is_named_in_the_disabled_reason() {
        let no_embed_key = format!("[ai.embedding]\n{UNSET_KEY}");
        let no_daily_key = format!("[ai.daily]\nmodel = \"m\"\n{UNSET_KEY}");
        let cases: [(&[&str], &str); 6] = [
            (&[], "[daily] is not configured"),
            (&[DAILY], "[daily] is set but [ai.embedding] is missing"),
            (
                &[DAILY, &no_embed_key],
                "[ai.embedding] has no API key — set $XUEWEN_TEST_UNSET_KEY or `api_key_env`",
            ),
            (
                &[DAILY, EMBEDDING],
                "[daily] is set but [ai.daily] is missing",
            ),
            (
                &[DAILY, EMBEDDING, "[ai.daily]\napi_key = \"k\"\n"],
                "[ai.daily] has no model — set `model` under [ai] or [ai.daily]",
            ),
            (
                &[DAILY, EMBEDDING, &no_daily_key],
                "[ai.daily] has no API key — set $XUEWEN_TEST_UNSET_KEY or `api_key_env`",
            ),
        ];
        for (sections, want) in cases {
            assert_eq!(reason(sections, ready().await).await, want, "{sections:?}");
        }
    }

    #[tokio::test]
    async fn a_complete_config_still_needs_the_vector_store() {
        let full = [
            DAILY,
            EMBEDDING,
            "[ai.daily]\nmodel = \"m\"\napi_key = \"k\"\n",
        ];
        assert_eq!(reason(&full, ready().await).await, "enabled");
        assert_eq!(
            reason(&full, Semantic::NotConfigured).await,
            "the vector store is unavailable (embedding API not configured (set [ai.embedding] and an API key))"
        );
    }

    #[tokio::test]
    async fn invalid_daily_values_are_errors_not_reasons() {
        let cfg: Config =
            toml::from_str(&format!("{BASE}[daily]\ncategories = []\n{EMBEDDING}")).unwrap();
        let (_dir, pool) = temp_pool().await;
        assert!(DailyService::try_from_config(&cfg, pool, &Semantic::NotConfigured).is_err());
    }
}
