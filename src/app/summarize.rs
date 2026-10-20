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
    // Say precisely why the service is off: an absent section, a
    // missing model, or a key whose env var came up empty.
    let Some(use_) = cfg.ai.summary.as_ref() else {
        return NOT_CONFIGURED.to_string();
    };
    let r = cfg.ai.resolve(use_);
    if r.model.is_none() {
        return "[ai.summary] has no model — set `model` under [ai] or [ai.summary]".to_string();
    }
    format!(
        "[ai.summary] has no API key — set ${} or `api_key_env`",
        r.api_key_env
    )
}
