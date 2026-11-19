use super::error::Error;
use super::App;
use crate::ingest::pipeline::{IdentifyOutcome, IngestCtx};
use crate::library::model::{Identifier, Paper};
use crate::metadata::resolve::ResolvedMetadata;

pub(crate) struct Identify<'a> {
    app: &'a App,
    ctx: &'a IngestCtx,
}

impl App {
    pub(crate) fn identify(&self) -> Result<Identify<'_>, Error> {
        let ingest = self.ingest().map_err(|e| match e {
            Error::Unavailable(_) => Error::Unavailable("identify not configured".into()),
            e => e,
        })?;
        Ok(Identify {
            app: self,
            ctx: ingest.ctx(),
        })
    }
}

impl Identify<'_> {
    pub(crate) async fn candidates(&self, query: &str) -> Result<Vec<ResolvedMetadata>, Error> {
        self.ctx
            .resolver
            .search_candidates(query)
            .await
            .map_err(Error::LookupFailed)
    }

    pub(crate) async fn lookup(&self, id: &Identifier) -> Result<Option<ResolvedMetadata>, Error> {
        self.ctx
            .resolver
            .lookup(id)
            .await
            .map_err(Error::LookupFailed)
    }

    pub(crate) async fn apply(
        &self,
        paper: &mut Paper,
        md: ResolvedMetadata,
    ) -> Result<IdentifyOutcome, Error> {
        let outcome = self.ctx.apply_match(paper, md).await?;
        if outcome == IdentifyOutcome::Applied {
            self.app.library_changed();
        }
        Ok(outcome)
    }
}
