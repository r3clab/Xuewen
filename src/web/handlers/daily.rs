use anyhow::Context as _;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;

use crate::app::App;
use crate::daily::store::DailyPaper;
use crate::web::error::ApiError;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/api/daily", get(daily_papers))
        .route("/api/daily/run", post(run_daily))
}

/// `date` is `None` until the first non-empty batch exists.
#[derive(Serialize)]
struct DailyResponse<'a> {
    date: Option<String>,
    papers: Vec<DailyPaperJson<'a>>,
}

/// The Glance widget reads `tldr` beside the summary it is taken from.
#[derive(Serialize)]
struct DailyPaperJson<'a> {
    #[serde(flatten)]
    paper: &'a DailyPaper,
    tldr: Option<&'a str>,
}

/// GET /api/daily — the latest non-empty daily batch for the Glance widget.
async fn daily_papers(State(app): State<App>) -> Result<Response, ApiError> {
    app.daily()?;
    let (date, papers) = match crate::daily::store::latest_batch(app.pool())
        .await
        .context("daily papers")?
    {
        Some((date, papers)) => (Some(date), papers),
        None => (None, Vec::new()),
    };
    let papers = papers
        .iter()
        .map(|paper| DailyPaperJson {
            paper,
            tldr: paper.tldr(),
        })
        .collect();
    Ok(Json(DailyResponse { date, papers }).into_response())
}

/// POST /api/daily/run — manual trigger; 202 started, 409 already running.
async fn run_daily(State(app): State<App>) -> Result<Response, ApiError> {
    let svc = app.daily()?;
    let today = crate::daily::scheduler::today();
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
