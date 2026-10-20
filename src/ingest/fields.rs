use anyhow::Result;
use sqlx::SqlitePool;

use super::pipeline::Outcome;
use crate::library::model::{Authors, Identifier, PaperMeta, PaperStatus};
use crate::library::papers;
use crate::metadata::resolve::ResolvedMetadata;

/// The raw inputs a resolution produces from a stored PDF, shared by ingest and
/// refresh. Consumed by `resolve_fields`.
pub(crate) struct ResolveInputs {
    pub(crate) ident: Identifier,
    pub(crate) provisional_title: Option<String>,
    pub(crate) extracted: Option<ResolvedMetadata>,
    pub(crate) resolution: Option<ResolvedMetadata>,
}

/// Decide the stored fields. A confident resolution yields `resolved` (with a
/// GROBID abstract backfilled if the source lacked one); otherwise `needs_review`,
/// enriched with GROBID's title/abstract/authors when present.
pub(crate) fn resolve_fields(
    provisional_title: Option<String>,
    extracted: Option<ResolvedMetadata>,
    ident: &Identifier,
    resolution: Option<ResolvedMetadata>,
) -> PaperMeta {
    let (ext_doi, ext_arxiv) = match ident {
        Identifier::Doi(d) => (Some(d.clone()), None),
        Identifier::Arxiv(a) => (None, Some(a.clone())),
        Identifier::None => (None, None),
    };
    match resolution {
        Some(md) => {
            let mut meta = paper_meta(md);
            meta.title = meta.title.or(provisional_title);
            meta.abstract_text = meta
                .abstract_text
                .or_else(|| extracted.and_then(|g| g.abstract_text));
            meta.doi = meta.doi.or(ext_doi);
            meta.arxiv_id = meta.arxiv_id.or(ext_arxiv);
            meta
        }
        None => match extracted {
            Some(g) => PaperMeta {
                title: g.title.or(provisional_title),
                abstract_text: g.abstract_text,
                authors: Authors(g.authors),
                venue: None,
                year: None,
                doi: ext_doi,
                arxiv_id: ext_arxiv,
                dblp_key: None,
                url: None,
                source: Some(g.source),
                status: PaperStatus::NeedsReview,
            },
            None => PaperMeta {
                title: provisional_title,
                abstract_text: None,
                authors: Authors::default(),
                venue: None,
                year: None,
                doi: ext_doi,
                arxiv_id: ext_arxiv,
                dblp_key: None,
                url: None,
                source: None,
                status: PaperStatus::NeedsReview,
            },
        },
    }
}

/// The direct mapping of a confident resolution: every field carried over,
/// status `Resolved`. The one place resolver output turns into stored
/// metadata — `resolve_fields` and `apply_match` layer their differing
/// fallbacks (provisional title, kept abstract, extracted-id merges) on
/// top rather than re-spelling the field list.
pub(crate) fn paper_meta(md: ResolvedMetadata) -> PaperMeta {
    PaperMeta {
        title: md.title,
        abstract_text: md.abstract_text,
        authors: Authors(md.authors),
        venue: md.venue,
        year: md.year,
        doi: md.doi,
        arxiv_id: md.arxiv_id,
        dblp_key: md.dblp_key,
        url: md.url,
        source: Some(md.source),
        status: PaperStatus::Resolved,
    }
}

/// After a UNIQUE violation on insert, find the row that won the race and map
/// it to the outcome the pre-insert checks would have produced.
pub(crate) async fn recover_unique_collision(
    pool: &SqlitePool,
    content_hash: &str,
    doi: Option<&str>,
    arxiv_id: Option<&str>,
) -> Result<Option<Outcome>> {
    if let Some(existing) = papers::find_by_hash(pool, content_hash).await? {
        return Ok(Some(Outcome::from_hash_hit(&existing)));
    }
    if let Some(existing) = papers::find_by_identifier(pool, doi, arxiv_id).await? {
        return Ok(Some(Outcome::from_identifier_hit(&existing)));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::library::trash;
    use crate::testutil::sample_paper;

    #[tokio::test]
    async fn recover_unique_collision_maps_all_cases() {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let pool = db::connect(&url).await.unwrap();
        let mut a = sample_paper("01890000-0000-7000-8000-0000000000aa", "h1");
        a.meta.doi = Some("10.1/x".into());
        papers::insert(&pool, &a).await.unwrap();

        // Hash collision with an active row → Duplicate.
        assert_eq!(
            recover_unique_collision(&pool, "h1", None, None)
                .await
                .unwrap(),
            Some(Outcome::Duplicate(a.id.clone()))
        );
        // Identifier collision with an active row → SameWork.
        assert_eq!(
            recover_unique_collision(&pool, "h2", Some("10.1/x"), None)
                .await
                .unwrap(),
            Some(Outcome::SameWork(a.id.clone()))
        );
        // Trashed row → InTrash for both shapes.
        trash::soft_delete(&pool, &a.id).await.unwrap();
        assert_eq!(
            recover_unique_collision(&pool, "h1", None, None)
                .await
                .unwrap(),
            Some(Outcome::InTrash(a.id.clone()))
        );
        assert_eq!(
            recover_unique_collision(&pool, "h2", Some("10.1/x"), None)
                .await
                .unwrap(),
            Some(Outcome::InTrash(a.id.clone()))
        );
        // No matching row → None (the violation was something else).
        assert_eq!(
            recover_unique_collision(&pool, "h3", Some("10.9/none"), None)
                .await
                .unwrap(),
            None
        );
    }
}
