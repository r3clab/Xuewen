use anyhow::Context as _;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};

use super::fetch_paper;
use crate::app::{App, Trash};
use crate::web::error::ApiError;
use serde::Deserialize;

pub(super) fn routes() -> Router<App> {
    Router::new().route("/api/papers/{id}/citations", post(parse_citations))
}

#[derive(Deserialize)]
struct ParseCitationsBody {
    references: Vec<String>,
}

/// POST /api/papers/{id}/citations — parse extracted reference strings into
/// structured fields: heuristics first (always available), then the
/// [ai.citations] LLM for entries heuristics couldn't parse (cached per
/// paper).
async fn parse_citations(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(body): Json<ParseCitationsBody>,
) -> Result<Response, ApiError> {
    let total: usize = body.references.iter().map(|r| r.len()).sum();
    if body.references.is_empty() || body.references.len() > 500 || total > 200_000 {
        return Err(ApiError::BadRequest(
            "references must be 1..=500 entries and under 200kB".into(),
        ));
    }
    // The paper's venue seeds the style vote's tie-breaker.
    let venue = fetch_paper(&app, &id, Trash::Allow, "parse_citations")
        .await?
        .meta
        .venue;
    let parsed = app
        .citations()
        .parse(&id, &body.references, venue.as_deref())
        .await
        .with_context(|| format!("parse_citations {id}"))?;
    Ok(Json(serde_json::json!({ "references": parsed })).into_response())
}
