use std::path::Path;

use anyhow::Context as _;

use super::error::Error;
use super::settings::PROXY_COOKIE;
use super::App;
use crate::ingest::import::{ImportError, Imported};
use crate::ingest::pipeline::Outcome;
use crate::ingest::refresh::{self, RefreshSummary, RefreshTarget};
use crate::ingest::watcher;
use crate::library::settings;

impl App {
    pub(crate) async fn ingest_path(&self, path: &Path) -> Result<Outcome, Error> {
        let outcome = self.ingest()?.ctx().ingest_file(path).await?;
        self.library_changed();
        Ok(outcome)
    }

    pub(crate) async fn ingest_upload(&self, name: &str, bytes: &[u8]) -> Result<Outcome, Error> {
        let outcome = self
            .ingest()?
            .ctx()
            .ingest_bytes(bytes, name, None)
            .await
            .map_err(ImportError::Ingest)?;
        self.library_changed();
        Ok(outcome)
    }

    pub(crate) async fn import(&self, input: &str) -> Result<Imported, Error> {
        let ingest = self.ingest()?;
        let cookie = settings::get(self.pool(), PROXY_COOKIE)
            .await
            .context("reading the proxy cookie")?;
        let imported = ingest.import(input, cookie.as_deref()).await?;
        if matches!(imported, Imported::Ingested(_)) {
            self.library_changed();
        }
        Ok(imported)
    }

    pub(crate) async fn refresh(&self, target: RefreshTarget) -> Result<RefreshSummary, Error> {
        let summary = refresh::run(self.ingest()?.ctx(), target).await;
        self.library_changed();
        Ok(summary?)
    }

    pub(crate) async fn watch(&self, inbox: &Path) -> Result<(), Error> {
        let ctx = self.ingest()?.ctx();
        Ok(watcher::run(ctx, inbox, self.tasks().token(), &|| self.library_changed()).await?)
    }
}
