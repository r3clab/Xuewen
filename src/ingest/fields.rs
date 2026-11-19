use anyhow::Result;
use sqlx::SqlitePool;

use crate::library::model::{
    canonical_arxiv, canonical_doi, Authors, Identifier, Paper, PaperMeta, PaperStatus,
};
use crate::library::papers;
use crate::metadata::resolve::ResolvedMetadata;

/// What resolving a stored PDF produced, shared by ingest and refresh.
pub(crate) struct ResolveInputs {
    /// The identifier the paper keeps whatever else resolves: a hint the
    /// caller vouches for (the import input the PDF was fetched by, or the
    /// paper's stored identifier on refresh), or one read from the PDF text
    /// that its resolver confirmed. An unconfirmed identifier from the text
    /// never gets here: page 1 can print a template placeholder or a cited
    /// work's DOI, and stored in the UNIQUE column it would turn every later
    /// paper carrying it into a false `SameWork`.
    pub(crate) ident: Identifier,
    pub(crate) provisional_title: Option<String>,
    pub(crate) extracted: Option<ResolvedMetadata>,
    pub(crate) resolution: Option<ResolvedMetadata>,
}

impl ResolveInputs {
    /// Decide the stored fields. A confident resolution yields `resolved`
    /// (with a GROBID abstract backfilled if the source lacked one); otherwise
    /// `needs_review`, enriched with GROBID's title/abstract/authors when
    /// present.
    pub(crate) fn into_meta(self) -> PaperMeta {
        let ResolveInputs {
            ident,
            provisional_title,
            extracted,
            resolution,
        } = self;
        let (ext_doi, ext_arxiv) = match ident {
            Identifier::Doi(d) => (Some(d), None),
            Identifier::Arxiv(a) => (None, Some(a)),
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
}

/// The direct mapping of a confident resolution: every field carried over,
/// identifiers canonicalised, status `Resolved`. The one place resolver
/// output turns into stored metadata — `into_meta` and `apply_match` layer
/// their differing fallbacks (provisional title, kept abstract,
/// extracted-id merges) on top rather than re-spelling the field list.
pub(crate) fn paper_meta(md: ResolvedMetadata) -> PaperMeta {
    PaperMeta {
        title: md.title,
        abstract_text: md.abstract_text,
        authors: Authors(md.authors),
        venue: md.venue,
        year: md.year,
        doi: md.doi.as_deref().map(canonical_doi),
        arxiv_id: md.arxiv_id.as_deref().map(canonical_arxiv),
        dblp_key: md.dblp_key,
        url: md.url,
        source: Some(md.source),
        status: PaperStatus::Resolved,
    }
}

/// The other paper holding `paper`'s bytes, else its DOI or arXiv id. After a
/// write of `paper` hit a UNIQUE constraint, that is the winner of the
/// concurrent ingest or identify it raced; `None` means the violation was
/// something else.
pub(crate) async fn unique_rival(pool: &SqlitePool, paper: &Paper) -> Result<Option<Paper>> {
    if let Some(rival) = papers::find_by_hash(pool, &paper.content_hash)
        .await?
        .filter(|p| p.id != paper.id)
    {
        return Ok(Some(rival));
    }
    papers::find_by_identifier(
        pool,
        paper.meta.doi.as_deref(),
        paper.meta.arxiv_id.as_deref(),
        Some(&paper.id),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{sample_paper, temp_pool};

    #[tokio::test]
    async fn unique_rival_finds_the_other_holder_of_bytes_or_identifier() {
        let (_dir, pool) = temp_pool().await;
        let mut a = sample_paper("01890000-0000-7000-8000-0000000000aa", "h1");
        a.meta.doi = Some("10.1/x".into());
        papers::insert(&pool, &a).await.unwrap();

        let same_bytes = sample_paper("01890000-0000-7000-8000-0000000000bb", "h1");
        assert_eq!(
            unique_rival(&pool, &same_bytes).await.unwrap().unwrap().id,
            a.id
        );
        let mut same_work = sample_paper("01890000-0000-7000-8000-0000000000bb", "h2");
        same_work.meta.doi = Some("10.1/x".into());
        assert_eq!(
            unique_rival(&pool, &same_work).await.unwrap().unwrap().id,
            a.id
        );
        // A paper is never its own rival: identify re-writes a stored row.
        assert!(unique_rival(&pool, &a).await.unwrap().is_none());
        let unrelated = sample_paper("01890000-0000-7000-8000-0000000000cc", "h3");
        assert!(unique_rival(&pool, &unrelated).await.unwrap().is_none());
    }

    #[test]
    fn paper_meta_stores_canonical_identifiers() {
        let meta = paper_meta(ResolvedMetadata {
            doi: Some("10.1145/AbC".into()),
            arxiv_id: Some("1706.03762v5".into()),
            source: "dblp".into(),
            ..Default::default()
        });
        assert_eq!(meta.doi.as_deref(), Some("10.1145/abc"));
        assert_eq!(meta.arxiv_id.as_deref(), Some("1706.03762"));
    }
}
