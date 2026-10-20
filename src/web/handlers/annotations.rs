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
    crate::annotations::validate_id(&annotation_id).map_err(ApiError::BadRequest)?;
    crate::annotations::validate_new(&body).map_err(ApiError::BadRequest)?;
    fetch_paper(&app, &paper_id, Trash::Allow, "put_annotation").await?;
    let a = app
        .annotations()
        .put(&paper_id, &annotation_id, body)
        .await
        .with_context(|| format!("put_annotation {paper_id}/{annotation_id}"))?;
    // The note feeds the `notes` search field; nudge the sweep so the
    // index catches up without waiting for its next poll.
    app.wake_search();
    Ok(Json(a).into_response())
}

/// DELETE /api/papers/{paper_id}/annotations/{annotation_id}
async fn delete_annotation(
    State(app): State<App>,
    Path((paper_id, annotation_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    if !app
        .annotations()
        .delete(&paper_id, &annotation_id)
        .await
        .with_context(|| format!("delete_annotation {paper_id}/{annotation_id}"))?
    {
        return Err(ApiError::NotFound);
    }
    app.wake_search();
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /api/papers/{id}/annotations — clear every mark on a paper. Answers
/// with the count rather than 204: "cleared 12 highlights" is worth surfacing.
async fn clear_annotations(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    fetch_paper(&app, &id, Trash::Allow, "clear_annotations").await?;
    let n = app
        .annotations()
        .delete_all(&id)
        .await
        .with_context(|| format!("clear_annotations {id}"))?;
    app.wake_search();
    Ok(Json(serde_json::json!({ "deleted": n })).into_response())
}
