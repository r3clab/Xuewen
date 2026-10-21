use super::error::Error;
use super::App;
use crate::config::Config;
use crate::summary::store;

pub(super) const NOT_CONFIGURED: &str = "[ai.summary] is not configured — nothing to do";

pub(crate) enum SummarizeScope {
    Paper(String),
    Pending,
    All,
}

impl App {
    pub(crate) async fn summarize(&self, scope: SummarizeScope) -> Result<usize, Error> {
        let svc = self.inner.summary.get().map_err(Error::Unavailable)?;
        let pool = self.pool();
        Ok(match scope {
            SummarizeScope::All => {
                store::clear(pool, None).await?;
                store::clear_failure(pool, None).await?;
                svc.sweep_all().await?
            }
            SummarizeScope::Paper(id) => {
                store::clear(pool, Some(&id)).await?;
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
