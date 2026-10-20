use std::path::Path;

use super::error::Error;
use super::settings::PROXY_COOKIE;
use super::App;
use crate::ingest::import::{ImportError, Imported};
use crate::ingest::pipeline::Outcome;
use crate::ingest::refresh::{self, RefreshSummary, RefreshTarget};
use crate::ingest::watcher;
use crate::library::settings;

impl From<Error> for ImportError {
    fn from(e: Error) -> Self {
        match e {
            Error::Unavailable(m) => ImportError::Unavailable(m),
            e => ImportError::Internal(e.into()),
        }
    }
}

impl App {
    pub(crate) async fn ingest_path(&self, path: &Path) -> Result<Outcome, Error> {
        let outcome = self.ingest()?.ctx().ingest_file(path).await;
        self.wake_search();
        Ok(outcome?)
    }

    pub(crate) async fn ingest_upload(&self, name: &str, bytes: &[u8]) -> Result<Outcome, Error> {
        let outcome = self.ingest()?.ctx().ingest_bytes(bytes, name, None).await;
        self.wake_search();
        Ok(outcome?)
    }

    pub(crate) async fn import(&self, input: &str) -> Result<Imported, ImportError> {
        let ingest = self.ingest()?;
        let cookie = settings::get(self.pool(), PROXY_COOKIE)
            .await
            .map_err(ImportError::Internal)?;
        let imported = ingest.import(input, cookie.as_deref()).await;
        if matches!(
            imported,
            Ok(Imported::Ingested(_)) | Err(ImportError::Ingest(_))
        ) {
            self.wake_search();
        }
        imported
    }

    pub(crate) async fn refresh(&self, target: RefreshTarget) -> Result<RefreshSummary, Error> {
        Ok(refresh::run(self.ingest()?.ctx(), target).await?)
    }

    pub(crate) async fn watch(&self, inbox: &Path) -> Result<(), Error> {
        Ok(watcher::run(self.ingest()?.ctx(), inbox, self.tasks().token()).await?)
    }
}
