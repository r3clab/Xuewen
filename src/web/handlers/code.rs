use anyhow::Context as _;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use ts_rs::TS;

use super::fetch_paper;
use crate::app::code::CloneMode;
use crate::app::{App, Trash};
use crate::web::error::ApiError;

pub(super) fn routes() -> Router<App> {
    Router::new().route(
        "/api/papers/{id}/code",
        get(get_paper_code)
            .put(set_paper_code)
            .delete(delete_paper_code),
    )
}

/// GET/PUT /api/papers/{id}/code response: `code` is the `paper_code` row
/// (the frontend's `PaperCodeStatus`), null when nothing is attached.
#[derive(Serialize, TS)]
#[ts(export)]
struct CodeAttachment {
    attached: bool,
    pub(crate) code: Option<crate::agent::store::PaperCode>,
}

#[derive(serde::Deserialize)]
struct CodeBody {
    repo_url: String,
}

/// Attach (or replace) a paper's code repo: 202 + row while the shallow
/// clone runs in the background; poll GET for the outcome.
async fn set_paper_code(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(body): Json<CodeBody>,
) -> Result<Response, ApiError> {
    app.agent()?;
    let paper = fetch_paper(&app, &id, Trash::Deny, "code").await?;
    let code = app
        .attach_code(&paper, &body.repo_url, CloneMode::Background)
        .await
        .map_err(ApiError::context("paper_code upsert"))?;
    Ok((
        StatusCode::ACCEPTED,
        Json(CodeAttachment {
            attached: true,
            code,
        }),
    )
        .into_response())
}

async fn get_paper_code(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let p = fetch_paper(&app, &id, Trash::Deny, "code").await?;
    let code = crate::agent::store::get_paper_code(app.pool(), &p.id)
        .await
        .context("paper_code get")?;
    Ok(Json(CodeAttachment {
        attached: code.is_some(),
        code,
    })
    .into_response())
}

async fn delete_paper_code(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let p = fetch_paper(&app, &id, Trash::Deny, "code").await?;
    app.detach_code(&p)
        .await
        .map_err(ApiError::context("paper_code delete"))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
