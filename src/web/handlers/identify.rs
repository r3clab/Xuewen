use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use super::fetch_paper;
use super::papers::{paper_refs, PaperDetail};
use crate::app::{App, Trash};
use crate::ingest::pipeline::IdentifyOutcome;
use crate::library::model::Identifier;
use crate::metadata::resolve::ResolvedMetadata;
use crate::web::error::ApiError;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/api/identify/search", get(identify_search))
        .route("/api/papers/{id}/identify", post(identify_paper))
}

/// A manual-identify candidate: a lossless wire mirror of `ResolvedMetadata`
/// (round-trips through POST /api/papers/{id}/identify without loss).
#[derive(Serialize, serde::Deserialize, TS)]
#[ts(export)]
struct Candidate {
    title: Option<String>,
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
    authors: Vec<String>,
    venue: Option<String>,
    year: Option<i64>,
    doi: Option<String>,
    arxiv_id: Option<String>,
    dblp_key: Option<String>,
    url: Option<String>,
    source: String,
}

impl From<&ResolvedMetadata> for Candidate {
    fn from(m: &ResolvedMetadata) -> Self {
        Self {
            title: m.title.clone(),
            abstract_text: m.abstract_text.clone(),
            authors: m.authors.clone(),
            venue: m.venue.clone(),
            year: m.year,
            doi: m.doi.clone(),
            arxiv_id: m.arxiv_id.clone(),
            dblp_key: m.dblp_key.clone(),
            url: m.url.clone(),
            source: m.source.clone(),
        }
    }
}

impl Candidate {
    /// Back to resolver metadata for the apply path.
    fn into_metadata(self) -> ResolvedMetadata {
        ResolvedMetadata {
            title: self.title,
            abstract_text: self.abstract_text,
            authors: self.authors,
            venue: self.venue,
            year: self.year,
            doi: self.doi,
            arxiv_id: self.arxiv_id,
            dblp_key: self.dblp_key,
            url: self.url,
            source: self.source,
        }
    }
}

#[derive(Deserialize)]
struct IdentifyQuery {
    q: Option<String>,
}

/// Ungated candidate search for manual identify (the user is the gate).
async fn identify_search(
    State(app): State<App>,
    Query(p): Query<IdentifyQuery>,
) -> Result<Response, ApiError> {
    let identify = app.identify()?;
    let q =
        p.q.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ApiError::BadRequest("missing query".into()))?;
    let cands = identify.candidates(q).await;
    let out: Vec<Candidate> = cands.iter().map(Candidate::from).collect();
    Ok(Json(out).into_response())
}

#[derive(Deserialize)]
struct IdentifyBody {
    doi: Option<String>,
    arxiv_id: Option<String>,
    candidate: Option<Candidate>,
}

#[derive(TS)]
#[ts(export, rename = "IdentifyBody", untagged)]
enum Selector {
    Doi { doi: String },
    ArxivId { arxiv_id: String },
    Candidate { candidate: Box<Candidate> },
}

impl IdentifyBody {
    fn selector(self) -> Option<Selector> {
        match (self.doi, self.arxiv_id, self.candidate) {
            (Some(doi), None, None) => Some(Selector::Doi { doi }),
            (None, Some(arxiv_id), None) => Some(Selector::ArxivId { arxiv_id }),
            (None, None, Some(candidate)) => Some(Selector::Candidate {
                candidate: Box::new(candidate),
            }),
            _ => None,
        }
    }
}

/// Apply a user-confirmed match: fetch authoritative metadata for a DOI or
/// arXiv id (or take a picked search candidate as-is), overwrite the paper's
/// metadata, and re-file. The user's confirmation replaces the confidence gate.
async fn identify_paper(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(body): Json<IdentifyBody>,
) -> Result<Response, ApiError> {
    let identify = app.identify()?;
    let selector = body.selector().ok_or_else(|| {
        ApiError::BadRequest("provide exactly one of doi, arxiv_id, candidate".into())
    })?;

    // Trash is Allow here only because `apply_match` itself answers a trashed
    // paper with the 409 below — the pipeline owns that policy.
    let mut paper = fetch_paper(&app, &id, Trash::Allow, "identify").await?;
    let md = match selector {
        Selector::Candidate { candidate } => Some(candidate.into_metadata()),
        Selector::Doi { doi } => identify.lookup(&Identifier::doi(doi)).await,
        Selector::ArxivId { arxiv_id } => identify.lookup(&Identifier::arxiv(arxiv_id)).await,
    };
    let Some(md) = md else {
        return Err(ApiError::NotFoundMsg(
            "identifier not found — not registered with Crossref/arXiv; try a title search".into(),
        ));
    };

    match identify
        .apply(&mut paper, md)
        .await
        .map_err(ApiError::context("identify apply"))?
    {
        IdentifyOutcome::Applied => {
            // The identify has already committed and the indexer been woken;
            // failing the response over chip enrichment would misreport a
            // completed identify as a 500 (and invite a client retry), so a
            // refs error degrades to empty tags/projects — unlike get_paper,
            // which propagates.
            let (tags, projects) = match paper_refs(app.pool(), &paper.id).await {
                Ok(refs) => refs,
                Err(e) => {
                    tracing::warn!("identify refs for {}: {e}", paper.id);
                    (Vec::new(), Vec::new())
                }
            };
            Ok(Json(PaperDetail::from(&paper).attach(tags, projects)).into_response())
        }
        IdentifyOutcome::SameWork(other) => Err(ApiError::Conflict {
            message: format!("same work as {other}"),
            id: Some(other),
        }),
        IdentifyOutcome::Trashed => Err(ApiError::Conflict {
            message: "paper is in the trash".into(),
            id: None,
        }),
    }
}
