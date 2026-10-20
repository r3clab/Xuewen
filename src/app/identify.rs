use super::error::Error;
use super::App;
use crate::db;
use crate::ingest::pipeline::{IdentifyOutcome, IngestCtx};
use crate::library::model::{Identifier, Paper};
use crate::library::papers;
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
    pub(crate) async fn candidates(&self, query: &str) -> Vec<ResolvedMetadata> {
        self.ctx.resolver.search_candidates(query).await
    }

    pub(crate) async fn lookup(&self, id: &Identifier) -> Option<ResolvedMetadata> {
        self.ctx.resolver.resolve(id, None).await
    }

    pub(crate) async fn apply(
        &self,
        paper: &mut Paper,
        md: ResolvedMetadata,
    ) -> Result<IdentifyOutcome, Error> {
        let (md_doi, md_arxiv) = (md.doi.clone(), md.arxiv_id.clone());
        let outcome = match self.ctx.apply_match(paper, md).await {
            Ok(outcome) => outcome,
            Err(e) => {
                recover_lost_race(&self.ctx.pool, e, md_doi.as_deref(), md_arxiv.as_deref()).await?
            }
        };
        if outcome == IdentifyOutcome::Applied {
            self.app.wake_search();
        }
        Ok(outcome)
    }
}

async fn recover_lost_race(
    pool: &sqlx::SqlitePool,
    e: anyhow::Error,
    doi: Option<&str>,
    arxiv_id: Option<&str>,
) -> Result<IdentifyOutcome, Error> {
    // Lost a race: something claimed this identifier between the guard and
    // the update. Report it as the conflict it is, mirroring ingest.
    if db::is_unique_violation(&e) {
        if let Ok(Some(existing)) = papers::find_by_identifier(pool, doi, arxiv_id).await {
            return Ok(IdentifyOutcome::SameWork(existing.id));
        }
    }
    Err(Error::Internal(e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{sample_paper, temp_pool};

    #[tokio::test]
    async fn a_unique_violation_is_reported_as_the_winning_paper() {
        let (_dir, pool) = temp_pool().await;
        let mut winner = sample_paper("01890000-0000-7000-8000-0000000000a1", "ha");
        winner.meta.doi = Some("10.1/x".into());
        papers::insert(&pool, &winner).await.unwrap();
        let mut loser = sample_paper("01890000-0000-7000-8000-0000000000b2", "hb");
        loser.meta.doi = Some("10.1/x".into());
        let e = papers::insert(&pool, &loser).await.unwrap_err();

        let outcome = recover_lost_race(&pool, e, Some("10.1/X"), None)
            .await
            .unwrap();
        assert_eq!(outcome, IdentifyOutcome::SameWork(winner.id));

        let other = anyhow::anyhow!("disk on fire");
        assert!(matches!(
            recover_lost_race(&pool, other, Some("10.1/x"), None).await,
            Err(Error::Internal(_))
        ));
    }
}
