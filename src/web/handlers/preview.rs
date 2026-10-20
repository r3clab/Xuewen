use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use ts_rs::TS;

use super::fetch_paper;
use super::papers::library_file;
use crate::app::{App, Trash};
use crate::web::error::ApiError;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/api/papers/{id}/preview", get(preview_meta))
        .route("/papers/{id}/preview/{page}", get(preview_page))
}

/// A paper's preview geometry: how many pages the picker should lay out, and
/// the shape of page one so the placeholders are correct before any image
/// loads.
#[derive(Serialize, TS)]
#[ts(export)]
struct PreviewMeta {
    pages: u32,
    page_width: f32,
    page_height: f32,
}

/// Map a render failure onto the wire. `Unrenderable` is 422 rather than 500
/// because nothing is broken — this PDF simply has no image to show, and it
/// is the one signal the picker needs to switch to its text card.
fn preview_error(e: crate::preview::PreviewError) -> ApiError {
    match e {
        crate::preview::PreviewError::Unrenderable => {
            ApiError::Unprocessable("the PDF could not be rendered".into())
        }
        crate::preview::PreviewError::PageOutOfRange => ApiError::NotFound,
    }
}

/// How many pages the picker should lay out, and page one's shape.
async fn preview_meta(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Json<PreviewMeta>, ApiError> {
    // Trash::Allow, like `pdf`: a paper trashed while the picker is open is
    // still a paper the picker is showing.
    let paper = fetch_paper(&app, &id, Trash::Allow, "preview").await?;
    let path = library_file(app.library_root(), &paper.rel_path).await?;
    let meta = app
        .preview()
        .meta(&path, &paper.content_hash)
        .await
        .map_err(preview_error)?;
    Ok(Json(PreviewMeta {
        pages: meta.pages,
        page_width: meta.width,
        page_height: meta.height,
    }))
}

/// One rendered page as a PNG.
async fn preview_page(
    State(app): State<App>,
    // The page number is parsed here rather than by a typed extractor: a
    // typed one rejects with axum's own body shape, and every error this API
    // returns is `{"error": "..."}`.
    Path((id, page)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let page: u32 = page
        .parse()
        .map_err(|_| ApiError::BadRequest("page must be a number".into()))?;
    let paper = fetch_paper(&app, &id, Trash::Allow, "preview").await?;
    let path = library_file(app.library_root(), &paper.rel_path).await?;
    let png = app
        .preview()
        .page_png(&path, &paper.content_hash, page)
        .await
        .map_err(preview_error)?;
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "image/png"),
            // The bytes only change when the file's hash does, but the URL is
            // keyed by paper id, so this is a bounded cache rather than an
            // immutable one.
            (axum::http::header::CACHE_CONTROL, "private, max-age=3600"),
        ],
        png,
    )
        .into_response())
}
