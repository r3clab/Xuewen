use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use super::fetch_paper;
use super::papers::{PaperDetail, paper_refs};
use crate::app::{App, Trash};
use crate::ingest::pipeline::IdentifyOutcome;
use crate::library::model::Identifier;
use crate::metadata::resolve::ResolvedMetadata;
use crate::web::error::ApiError;
use crate::web::extract::{ApiJson, ApiPath, ApiQuery};
use serde::Deserialize;
use ts_rs::TS;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/api/identify/search", get(identify_search))
        .route("/api/papers/{id}/identify", post(identify_paper))
}

#[derive(Deserialize)]
struct IdentifyQuery {
    q: Option<String>,
}

/// Ungated candidate search for manual identify (the user is the gate).
async fn identify_search(
    State(app): State<App>,
    ApiQuery(p): ApiQuery<IdentifyQuery>,
) -> Result<Json<Vec<ResolvedMetadata>>, ApiError> {
    let identify = app.identify()?;
    let q =
        p.q.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ApiError::BadRequest("missing query".into()))?;
    Ok(Json(identify.candidates(q).await?))
}

/// The match to apply: an identifier to look up, or a picked search candidate
/// taken as-is.
#[derive(Deserialize, TS)]
#[serde(
    untagged,
    deny_unknown_fields,
    expecting = "provide exactly one of doi, arxiv_id, candidate"
)]
#[ts(export, rename = "IdentifyBody")]
enum Selector {
    Doi { doi: String },
    ArxivId { arxiv_id: String },
    Candidate { candidate: Box<ResolvedMetadata> },
}

/// Apply a user-confirmed match: fetch authoritative metadata for a DOI or
/// arXiv id (or take a picked search candidate as-is), overwrite the paper's
/// metadata, and re-file. The user's confirmation replaces the confidence gate.
async fn identify_paper(
    State(app): State<App>,
    ApiPath(id): ApiPath<String>,
    ApiJson(selector): ApiJson<Selector>,
) -> Result<Response, ApiError> {
    let identify = app.identify()?;
    // Trash is Allow here only because `apply_match` itself answers a trashed
    // paper with the 409 below — the pipeline owns that policy.
    let mut paper = fetch_paper(&app, &id, Trash::Allow, "identify").await?;
    let md = match selector {
        Selector::Candidate { candidate } => Some(*candidate),
        Selector::Doi { doi } => identify.lookup(&Identifier::doi(doi)).await?,
        Selector::ArxivId { arxiv_id } => identify.lookup(&Identifier::arxiv(arxiv_id)).await?,
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
