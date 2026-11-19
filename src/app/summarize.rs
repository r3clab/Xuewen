use super::error::Error;
use super::App;
use crate::config::Config;
use crate::library::papers;
use crate::summary::store;

pub(super) const NOT_CONFIGURED: &str = "[ai.summary] is not configured — nothing to do";

pub(crate) enum SummarizeScope {
    Paper(String),
    Pending,
    All,
}

impl App {
    /// A regenerated summary replaces the stored one only once it exists, so
    /// a failing model never costs a paper the summary it already has.
    pub(crate) async fn summarize(&self, scope: SummarizeScope) -> Result<usize, Error> {
        let svc = self.inner.summary.get().map_err(Error::Unavailable)?;
        let pool = self.pool();
        Ok(match scope {
            SummarizeScope::All => {
                store::clear_failure(pool, None).await?;
                let mut written = 0;
                for paper in papers::all(pool).await? {
                    match svc.summarize_one(&paper.id).await {
                        Ok(stored) => written += usize::from(stored),
                        Err(e) => tracing::warn!("summary generation for {}: {e:#}", paper.id),
                    }
                }
                written
            }
            SummarizeScope::Paper(id) => {
                store::clear_failure(pool, Some(&id)).await?;
                usize::from(svc.summarize_one(&id).await?)
            }
            SummarizeScope::Pending => svc.sweep_all().await?,
        })
    }
}

pub(super) fn disabled_reason(cfg: &Config) -> String {
    match cfg.ai.summary.as_ref() {
        None => NOT_CONFIGURED.to_string(),
        Some(use_) => cfg.ai.resolve(use_).unusable_reason("ai.summary"),
    }
}
