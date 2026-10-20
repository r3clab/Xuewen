use anyhow::Context as _;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, put};
use axum::{Json, Router};

use super::fetch_paper;
use crate::app::{App, Trash};
use crate::library::projects::{self, ProjectName};
use crate::web::error::ApiError;
use serde::Deserialize;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/api/projects", get(list_projects).post(create_project))
        .route(
            "/api/projects/{id}",
            patch(update_project).delete(delete_project),
        )
        .route(
            "/api/papers/{paper_id}/projects/{project_id}",
            put(add_paper_to_project).delete(remove_paper_from_project),
        )
}

#[derive(Deserialize)]
struct CreateProjectBody {
    name: String,
}

#[derive(Deserialize)]
struct UpdateProjectBody {
    name: Option<String>,
}

async fn list_projects(State(app): State<App>) -> Result<Response, ApiError> {
    let list = projects::list(app.pool()).await.context("list_projects")?;
    Ok(Json(list).into_response())
}

async fn create_project(
    State(app): State<App>,
    Json(body): Json<CreateProjectBody>,
) -> Result<Response, ApiError> {
    let name = ProjectName::parse(&body.name)?;
    match projects::create(app.pool(), &name).await {
        Ok(project) => Ok((StatusCode::CREATED, Json(project)).into_response()),
        Err(e) => Err(ApiError::conflict_or_internal(
            e,
            "create_project",
            "a project with that name already exists",
        )),
    }
}

async fn update_project(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(body): Json<UpdateProjectBody>,
) -> Result<Response, ApiError> {
    // Merge: an omitted/blank name keeps the old one. Blankness is decided
    // by `ProjectName::parse` with `str::trim` (all Unicode whitespace), not
    // SQL `TRIM` (spaces only) — a tab-only name must keep the old name, not
    // be stored.
    let name = body
        .name
        .as_deref()
        .and_then(|n| ProjectName::parse(n).ok());
    match projects::update(app.pool(), &id, name.as_ref()).await {
        Ok(Some(p)) => Ok(Json(p).into_response()),
        Ok(None) => Err(ApiError::NotFound),
        Err(e) => Err(ApiError::conflict_or_internal(
            e,
            "update_project",
            "a project with that name already exists",
        )),
    }
}

async fn delete_project(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    if !projects::delete(app.pool(), &id)
        .await
        .context("delete_project")?
    {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn add_paper_to_project(
    State(app): State<App>,
    Path((paper_id, project_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    // Pre-check both ids so a bad request is a clean 404 (not an FK error).
    fetch_paper(&app, &paper_id, Trash::Allow, "membership").await?;
    projects::get(app.pool(), &project_id)
        .await
        .context("membership project lookup")?
        .ok_or(ApiError::NotFound)?;
    projects::add_paper(app.pool(), &paper_id, &project_id)
        .await
        .context("add membership")?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn remove_paper_from_project(
    State(app): State<App>,
    Path((paper_id, project_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    if !projects::remove_paper(app.pool(), &paper_id, &project_id)
        .await
        .context("remove membership")?
    {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}
