use anyhow::Context as _;

use super::error::Error;
use super::App;
use crate::library::settings;

pub(crate) const PROXY_COOKIE: &str = "proxy_cookie";

pub(crate) struct ProxyCookieStatus {
    pub(crate) set: bool,
    pub(crate) updated_at: Option<String>,
}

impl App {
    pub(crate) async fn set_proxy_cookie(&self, raw: &str) -> Result<(), Error> {
        let cookie = raw.trim();
        if cookie.is_empty() {
            return Err(Error::Invalid("empty cookie".into()));
        }
        settings::set(self.pool(), PROXY_COOKIE, cookie).await?;
        Ok(())
    }

    pub(crate) async fn clear_proxy_cookie(&self) -> Result<(), Error> {
        settings::delete(self.pool(), PROXY_COOKIE).await?;
        Ok(())
    }

    pub(crate) async fn proxy_cookie_status(&self) -> Result<ProxyCookieStatus, Error> {
        let set = settings::get(self.pool(), PROXY_COOKIE)
            .await
            .context("settings proxy cookie")?
            .is_some();
        let updated_at = settings::updated_at(self.pool(), PROXY_COOKIE)
            .await
            .context("settings cookie timestamp")?;
        Ok(ProxyCookieStatus { set, updated_at })
    }
}
