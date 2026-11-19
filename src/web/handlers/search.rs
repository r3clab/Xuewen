use anyhow::Context as _;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use super::papers::{PaperSummary, attach_row_extras};
use crate::app::App;
use crate::search::{SearchHit, SearchResults};
use crate::web::error::ApiError;
use crate::web::extract::ApiQuery;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/api/search", get(search_papers))
        .route("/api/search/status", get(search_status))
}

#[derive(Deserialize)]
struct SearchParams {
    q: Option<String>,
    fields: Option<String>,
    engines: Option<String>,
    status: Option<crate::library::model::PaperStatus>,
    project: Option<String>,
    tag: Option<String>,
    starred: Option<bool>,
}

/// Hybrid search. `fields`/`engines` are CSV lists; absent or unknown-only
/// values fall back to "all" (mirrors the whitelisting style elsewhere).
async fn search_papers(
    State(app): State<App>,
    ApiQuery(p): ApiQuery<SearchParams>,
) -> Result<Response, ApiError> {
    let svc = app.search()?;
    let (keyword, semantic) = parse_engines(p.engines.as_deref());
    let req = crate::search::SearchRequest::assemble(
        app.pool(),
        p.q.as_deref().unwrap_or(""),
        crate::search::RequestOverrides {
            keyword,
            semantic,
            fields: p.fields,
            status: p.status,
            project: p.project,
            tag: p.tag,
            starred: p.starred,
        },
    )
    .await
    .context("search request")?;
    let out = svc.search(&req).await.context("search")?;
    // Same enrichment as list_papers: without it, search rows serialize
    // empty tags/projects and the table's chips vanish whenever a query is
    // active.
    let mut summaries: Vec<PaperSummary> = out
        .results
        .iter()
        .map(|hit| PaperSummary::from(&hit.paper))
        .collect();
    attach_row_extras(app.pool(), &mut summaries)
        .await
        .context("search row extras")?;
    let body = SearchResults {
        semantic: out.semantic,
        results: summaries
            .into_iter()
            .zip(out.results)
            .map(|(paper, hit)| SearchHit {
                paper,
                matched: hit.matched,
            })
            .collect(),
    };
    Ok(Json(body).into_response())
}

fn parse_engines(csv: Option<&str>) -> (bool, bool) {
    let (mut keyword, mut semantic) = (false, false);
    for part in csv.unwrap_or("").split(',').map(str::trim) {
        match part {
            "keyword" => keyword = true,
            "semantic" => semantic = true,
            _ => {}
        }
    }
    if keyword || semantic {
        (keyword, semantic)
    } else {
        (true, true) // absent/unknown-only -> both
    }
}

async fn search_status(State(app): State<App>) -> Result<Response, ApiError> {
    let svc = app.search()?;
    let st = svc.status().await.context("search status")?;
    Ok(Json(st).into_response())
}
