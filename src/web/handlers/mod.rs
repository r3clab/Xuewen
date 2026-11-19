mod annotations;
mod chat;
mod citations;
mod code;
mod daily;
mod identify;
mod import;
mod papers;
mod preview;
mod projects;
mod search;
mod settings;
mod tags;
mod translate;

use axum::http::{HeaderMap, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

use super::error::ApiError;
use crate::app::{App, Trash};

pub(super) fn router() -> Router<App> {
    Router::new()
        .route("/api/health", get(health))
        .merge(papers::routes())
        .merge(import::routes())
        .merge(identify::routes())
        .merge(preview::routes())
        .merge(settings::routes())
        .merge(projects::routes())
        .merge(tags::routes())
        .merge(search::routes())
        .merge(daily::routes())
        .merge(chat::routes())
        .merge(citations::routes())
        .merge(annotations::routes())
        .merge(code::routes())
        .merge(translate::routes())
        .fallback(unrouted)
}

/// Liveness: answers without touching the database, so a slow query never
/// gets the process restarted.
async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

/// Under the API's prefixes an unmatched path is a missing endpoint, and
/// answers in the API's error shape; anywhere else it is the SPA's to serve.
async fn unrouted(uri: Uri, headers: HeaderMap) -> Response {
    match uri.path().split('/').nth(1) {
        Some("api" | "papers") => ApiError::NotFoundMsg("no such endpoint".into()).into_response(),
        _ => super::assets::static_handler(uri, headers).await,
    }
}

pub(super) async fn fetch_paper(
    app: &App,
    id: &str,
    trash: Trash,
    ctx: &'static str,
) -> Result<crate::library::model::Paper, ApiError> {
    app.paper(id, trash)
        .await
        .map_err(|e| ApiError::context(format!("{ctx} paper lookup"))(e))
}
