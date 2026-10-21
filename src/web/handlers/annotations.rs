use anyhow::Context as _;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};

use super::fetch_paper;
use crate::annotations::NewAnnotation;
use crate::app::{App, Trash};
use crate::web::error::ApiError;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route(
            "/api/papers/{id}/annotations",
            get(list_annotations).delete(clear_annotations),
        )
        // PUT (not POST): the reader's plugin mints the annotation id, so the
        // id addresses the row and a retried save is idempotent. Deliberately
        // no PATCH: a partial update of the projection columns would desync
        // them from the authoritative payload, which the reader replays alone.
        .route(
            "/api/papers/{paper_id}/annotations/{annotation_id}",
            put(put_annotation).delete(delete_annotation),
        )
}

/// GET /api/papers/{id}/annotations — every mark on a paper, in reading order.
/// The reader replays these into the annotation plugin on open.
async fn list_annotations(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    fetch_paper(&app, &id, Trash::Allow, "list_annotations").await?;
    let items = app
        .annotations()
        .list(&id)
        .await
        .with_context(|| format!("list_annotations {id}"))?;
    Ok(Json(items).into_response())
}

/// PUT /api/papers/{paper_id}/annotations/{annotation_id} — create or replace.
/// Idempotent on purpose: the annotation id is minted by the reader's plugin,
/// so a retried save re-sends the same address instead of duplicating a mark.
async fn put_annotation(
    State(app): State<App>,
    Path((paper_id, annotation_id)): Path<(String, String)>,
    Json(body): Json<NewAnnotation>,
) -> Result<Response, ApiError> {
    let a = app
        .put_annotation(&paper_id, &annotation_id, body)
        .await
        .map_err(ApiError::context(format!(
            "put_annotation {paper_id}/{annotation_id}"
        )))?;
    Ok(Json(a).into_response())
}

/// DELETE /api/papers/{paper_id}/annotations/{annotation_id}
async fn delete_annotation(
    State(app): State<App>,
    Path((paper_id, annotation_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    app.delete_annotation(&paper_id, &annotation_id)
        .await
        .map_err(ApiError::context(format!(
            "delete_annotation {paper_id}/{annotation_id}"
        )))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /api/papers/{id}/annotations — clear every mark on a paper. Answers
/// with the count rather than 204: "cleared 12 highlights" is worth surfacing.
async fn clear_annotations(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let n = app
        .clear_annotations(&id)
        .await
        .map_err(ApiError::context(format!("clear_annotations {id}")))?;
    Ok(Json(serde_json::json!({ "deleted": n })).into_response())
}
