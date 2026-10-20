use anyhow::Context as _;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;

use crate::app::App;
use crate::web::error::ApiError;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/api/daily", get(daily_papers))
        .route("/api/daily/run", post(run_daily))
}

/// One paper in the daily-recommendations response (Glance widget input).
#[derive(Serialize)]
struct DailyPaperDto {
    rank: i64,
    arxiv_id: String,
    title: String,
    authors: Vec<String>,
    #[serde(rename = "abstract")]
    abstract_text: String,
    categories: Vec<String>,
    score: f64,
    tldr: Option<String>,
    abs_url: String,
    pdf_url: String,
    summary: Option<crate::summary::Summary>,
    code_url: Option<String>,
}

impl From<&crate::daily::store::DailyPaper> for DailyPaperDto {
    fn from(p: &crate::daily::store::DailyPaper) -> Self {
        Self {
            rank: p.rank,
            arxiv_id: p.arxiv_id.clone(),
            title: p.title.clone(),
            authors: p.authors.clone(),
            abstract_text: p.abstract_text.clone(),
            categories: p.categories.clone(),
            score: p.score,
            tldr: p.tldr.clone(),
            abs_url: p.abs_url.clone(),
            pdf_url: p.pdf_url.clone(),
            summary: p.summary.clone(),
            code_url: p.code_url.clone(),
        }
    }
}

/// `date` is `None` until the first non-empty batch exists.
#[derive(Serialize)]
struct DailyResponse {
    date: Option<String>,
    papers: Vec<DailyPaperDto>,
}

/// GET /api/daily — the latest non-empty daily batch for the Glance widget.
async fn daily_papers(State(app): State<App>) -> Result<Response, ApiError> {
    app.daily()?;
    let resp = match crate::daily::store::latest_batch(app.pool())
        .await
        .context("daily papers")?
    {
        Some((date, papers)) => DailyResponse {
            date: Some(date),
            papers: papers.iter().map(DailyPaperDto::from).collect(),
        },
        None => DailyResponse {
            date: None,
            papers: Vec::new(),
        },
    };
    Ok(Json(resp).into_response())
}

/// POST /api/daily/run — manual trigger; 202 started, 409 already running.
async fn run_daily(State(app): State<App>) -> Result<Response, ApiError> {
    let svc = app.daily()?;
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    if !svc.spawn_run(today, app.tasks()) {
        return Err(ApiError::Conflict {
            message: "a daily run is already in flight".into(),
            id: None,
        });
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({"status": "started"})),
    )
        .into_response())
}
