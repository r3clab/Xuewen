use anyhow::Context as _;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};

use super::fetch_paper;
use super::papers::{attach_row_extras, PaperSummary};
use crate::app::{App, Trash};
use crate::citations::matching::{self, CitedReference};
use crate::citations::StructuredReference;
use crate::library::papers;
use crate::web::error::ApiError;
use crate::web::extract::{ApiJson, ApiPath};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/api/papers/{id}/citations", post(parse_citations))
        .route("/api/citations/match", post(match_citations))
}

const MAX_REFERENCES: usize = 500;
const MAX_TOTAL_BYTES: usize = 200_000;

/// The bound both citation endpoints put on one paper's bibliography.
fn check_size<'a>(texts: impl ExactSizeIterator<Item = &'a str>) -> Result<(), ApiError> {
    let count = texts.len();
    let total: usize = texts.map(str::len).sum();
    if count == 0 || count > MAX_REFERENCES || total > MAX_TOTAL_BYTES {
        return Err(ApiError::BadRequest(format!(
            "references must be 1..={MAX_REFERENCES} entries and under {}kB",
            MAX_TOTAL_BYTES / 1000
        )));
    }
    Ok(())
}

#[derive(Deserialize, TS)]
#[ts(export)]
struct ParseCitationsBody {
    references: Vec<String>,
}

/// Index-aligned with the request's `references`; `null` where neither the
/// heuristics nor the LLM produced fields.
#[derive(Serialize, TS)]
#[ts(export)]
struct ParsedCitations {
    references: Vec<Option<StructuredReference>>,
}

/// POST /api/papers/{id}/citations — parse extracted reference strings into
/// structured fields: heuristics first (always available), then the
/// [ai.citations] LLM for entries heuristics couldn't parse (cached per
/// paper).
async fn parse_citations(
    State(app): State<App>,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<ParseCitationsBody>,
) -> Result<Response, ApiError> {
    check_size(body.references.iter().map(String::as_str))?;
    // The paper's venue seeds the style vote's tie-breaker.
    let venue = fetch_paper(&app, &id, Trash::Allow, "parse_citations")
        .await?
        .meta
        .venue;
    let references = app
        .citations()
        .parse(&id, &body.references, venue.as_deref())
        .await
        .with_context(|| format!("parse_citations {id}"))?;
    Ok(Json(ParsedCitations { references }).into_response())
}

#[derive(Deserialize, TS)]
#[ts(export)]
struct MatchCitationsBody {
    references: Vec<CitedReference>,
}

/// Index-aligned with the request's `references`: the library paper each one
/// cites, or `null`.
#[derive(Serialize, TS)]
#[ts(export)]
struct CitationMatches {
    matches: Vec<Option<PaperSummary>>,
}

/// POST /api/citations/match — which of a bibliography's entries the library
/// already holds.
async fn match_citations(
    State(app): State<App>,
    ApiJson(body): ApiJson<MatchCitationsBody>,
) -> Result<Response, ApiError> {
    check_size(body.references.iter().map(|r| r.text.as_str()))?;
    let library = papers::all(app.pool()).await.context("match_citations")?;
    let matched = matching::match_library(&body.references, &library);
    let mut rows: Vec<PaperSummary> = matched
        .iter()
        .flatten()
        .map(|p| PaperSummary::from(*p))
        .collect();
    attach_row_extras(app.pool(), &mut rows)
        .await
        .context("match_citations row extras")?;
    let mut rows = rows.into_iter();
    let matches = matched
        .iter()
        .map(|m| m.and_then(|_| rows.next()))
        .collect();
    Ok(Json(CitationMatches { matches }).into_response())
}
