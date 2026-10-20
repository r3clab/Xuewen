use anyhow::Context as _;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, put};
use axum::{Json, Router};

use super::fetch_paper;
use super::papers::TagRef;
use crate::app::{App, Trash};
use crate::library::tags::{self, TagName};
use crate::web::error::ApiError;
use serde::Deserialize;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/api/tags", get(list_tags))
        .route("/api/tags/{id}", patch(rename_tag).delete(delete_tag))
        .route("/api/papers/{paper_id}/tags", put(add_paper_tag))
        .route(
            "/api/papers/{paper_id}/tags/{tag_id}",
            delete(remove_paper_tag),
        )
}

async fn list_tags(State(app): State<App>) -> Result<Response, ApiError> {
    let tags = tags::list(app.pool()).await.context("list_tags")?;
    Ok(Json(tags).into_response())
}

#[derive(Deserialize)]
struct TagNameBody {
    name: String,
}

async fn add_paper_tag(
    State(app): State<App>,
    Path(paper_id): Path<String>,
    Json(body): Json<TagNameBody>,
) -> Result<Response, ApiError> {
    let name = TagName::parse(&body.name)?;
    // Pre-check the paper so a bad id is a clean 404, not an FK-violation 500.
    fetch_paper(&app, &paper_id, Trash::Allow, "add_paper_tag").await?;
    let t = tags::add_paper(app.pool(), &paper_id, &name)
        .await
        .context("add_paper_tag")?;
    Ok(Json(TagRef::from(t)).into_response())
}

async fn remove_paper_tag(
    State(app): State<App>,
    Path((paper_id, tag_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    if !tags::remove_paper(app.pool(), &paper_id, &tag_id)
        .await
        .context("remove_paper_tag")?
    {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn rename_tag(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(body): Json<TagNameBody>,
) -> Result<Response, ApiError> {
    let name = TagName::parse(&body.name)?;
    match tags::rename(app.pool(), &id, &name).await {
        Ok(Some(t)) => Ok(Json(TagRef::from(t)).into_response()),
        Ok(None) => Err(ApiError::NotFound),
        Err(e) => Err(ApiError::conflict_or_internal(
            e,
            "rename_tag",
            "a tag with that name already exists",
        )),
    }
}

async fn delete_tag(State(app): State<App>, Path(id): Path<String>) -> Result<Response, ApiError> {
    if !tags::delete(app.pool(), &id).await.context("delete_tag")? {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}
