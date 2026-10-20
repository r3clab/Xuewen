use anyhow::Context as _;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};

use super::papers::{attach_row_extras, PaperSummary};
use crate::app::App;
use crate::web::error::ApiError;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/api/search", get(search_papers))
        .route("/api/search/status", get(search_status))
}

/// Why a paper matched a search query.
#[derive(Serialize, TS)]
#[ts(export)]
struct SearchMatch {
    #[ts(type = "\"keyword\" | \"semantic\" | \"both\"")]
    engine: String,
    field: String,
    /// HTML-safe: escaped text with <mark> highlights only.
    snippet: String,
    page: Option<i64>,
}

#[derive(Serialize, TS)]
#[ts(export, rename = "SearchResultItem")]
struct SearchResult {
    paper: PaperSummary,
    #[serde(rename = "match")]
    match_info: SearchMatch,
}

#[derive(Serialize, TS)]
#[ts(export)]
struct SemanticAvailability {
    available: bool,
    reason: Option<String>,
}

#[derive(Serialize, TS)]
#[ts(export)]
struct SearchResponse {
    semantic: SemanticAvailability,
    results: Vec<SearchResult>,
}

#[derive(Serialize, TS)]
#[ts(export)]
struct TierCounts {
    indexed: i64,
    pending: i64,
    failed: i64,
}

#[derive(Serialize, TS)]
#[ts(export)]
struct SearchStatus {
    fts: TierCounts,
    vectors: TierCounts,
    semantic_available: bool,
    reason: Option<String>,
}

#[derive(Deserialize)]
struct SearchParams {
    q: Option<String>,
    fields: Option<String>,
    engines: Option<String>,
    status: Option<String>,
    project: Option<String>,
    tag: Option<String>,
    starred: Option<bool>,
}

/// Hybrid search. `fields`/`engines` are CSV lists; absent or unknown-only
/// values fall back to "all" (mirrors the whitelisting style elsewhere).
async fn search_papers(
    State(app): State<App>,
    Query(p): Query<SearchParams>,
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
        .map(|(paper, _)| PaperSummary::from(paper))
        .collect();
    attach_row_extras(app.pool(), &mut summaries)
        .await
        .context("search row extras")?;
    let results: Vec<SearchResult> = summaries
        .into_iter()
        .zip(out.results.iter())
        .map(|(paper, (_, m))| SearchResult {
            paper,
            match_info: SearchMatch {
                engine: m.engine.as_str().to_string(),
                field: m.field.as_str().to_string(),
                snippet: m.snippet.clone(),
                page: m.page,
            },
        })
        .collect();
    Ok(Json(SearchResponse {
        semantic: SemanticAvailability {
            available: out.semantic.available,
            reason: out.semantic.reason,
        },
        results,
    })
    .into_response())
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
    Ok(Json(SearchStatus {
        fts: TierCounts {
            indexed: st.fts.indexed,
            pending: st.fts.pending,
            failed: st.fts.failed,
        },
        vectors: TierCounts {
            indexed: st.vectors.indexed,
            pending: st.vectors.pending,
            failed: st.vectors.failed,
        },
        semantic_available: st.semantic_available,
        reason: st.reason,
    })
    .into_response())
}
