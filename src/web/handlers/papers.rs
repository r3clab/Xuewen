use anyhow::Context as _;
use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use super::fetch_paper;
use crate::app::{App, Trash};
use crate::library::export;
use crate::library::model::{Paper, PaperStatus};
use crate::library::papers::{self, PaperName};
use crate::library::{projects, tags};
use crate::web::error::ApiError;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/api/papers", get(list_papers))
        .route("/api/papers/export", get(export_papers))
        .route("/api/papers/{id}", get(get_paper).delete(delete_paper))
        .route("/api/papers/{id}/restore", post(restore_paper))
        .route("/api/papers/{id}/export", get(export_paper))
        .route("/api/stats", get(stats))
        .route("/papers/{id}/pdf", get(pdf))
        .route(
            "/api/papers/{id}/star",
            put(star_paper).delete(unstar_paper),
        )
        .route("/api/papers/{id}/name", patch(set_paper_name))
}

#[derive(Deserialize)]
struct ListParams {
    q: Option<String>,
    status: Option<String>,
    sort: Option<String>,
    project: Option<String>,
    tag: Option<String>,
    starred: Option<bool>,
}

impl ListParams {
    /// The shared filter portion (`q`/`sort` travel separately into
    /// `papers::list`).
    fn filter(&self) -> papers::PaperFilter {
        papers::PaperFilter {
            status: self.status.clone(),
            project: self.project.clone(),
            tag: self.tag.clone(),
            starred: self.starred,
        }
    }
}

/// A tag reference as embedded in a paper row/detail.
#[derive(Serialize, Clone, TS)]
#[ts(export, rename = "Tag")]
pub(super) struct TagRef {
    id: String,
    name: String,
}

impl From<crate::library::tags::Tag> for TagRef {
    fn from(t: crate::library::tags::Tag) -> Self {
        Self {
            id: t.id,
            name: t.name,
        }
    }
}

/// A project reference as embedded in a paper row/detail.
#[derive(Serialize, Clone, TS)]
#[ts(export)]
pub(super) struct ProjectRef {
    id: String,
    name: String,
}

impl From<crate::library::projects::Project> for ProjectRef {
    fn from(p: crate::library::projects::Project) -> Self {
        Self {
            id: p.id,
            name: p.name,
        }
    }
}

/// A paper for the list view (no abstract, to keep the payload light).
#[derive(Serialize, TS)]
#[ts(export)]
pub(super) struct PaperSummary {
    id: String,
    /// Manual "known as" name: the system/tool the paper proposes (e.g. "RVSpec").
    name: Option<String>,
    title: Option<String>,
    authors: Vec<String>,
    venue: Option<String>,
    year: Option<i64>,
    doi: Option<String>,
    arxiv_id: Option<String>,
    dblp_key: Option<String>,
    cite_key: Option<String>,
    url: Option<String>,
    source: Option<String>,
    status: PaperStatus,
    added_at: String,
    starred: bool,
    tags: Vec<TagRef>,
    projects: Vec<ProjectRef>,
}

impl From<&Paper> for PaperSummary {
    fn from(p: &Paper) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            title: p.meta.title.clone(),
            authors: p.meta.authors.0.clone(),
            venue: p.meta.venue.clone(),
            year: p.meta.year,
            doi: p.meta.doi.clone(),
            arxiv_id: p.meta.arxiv_id.clone(),
            dblp_key: p.meta.dblp_key.clone(),
            cite_key: p.cite_key.clone(),
            url: p.meta.url.clone(),
            source: p.meta.source.clone(),
            status: p.meta.status,
            added_at: p.added_at.clone(),
            starred: p.starred,
            tags: Vec::new(),
            projects: Vec::new(),
        }
    }
}

/// A paper for the detail view: the summary fields plus the abstract. The
/// summary's `tags`/`projects` carry the paper's tag and project memberships.
#[derive(Serialize, TS)]
#[ts(export)]
pub(super) struct PaperDetail {
    #[serde(flatten)]
    summary: PaperSummary,
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
    /// LLM-generated structured summary (JSON key "summary"); absent until generated.
    #[serde(rename = "summary", skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    ai_summary: Option<crate::summary::Summary>,
}

impl From<&Paper> for PaperDetail {
    fn from(p: &Paper) -> Self {
        Self {
            summary: PaperSummary::from(p),
            abstract_text: p.meta.abstract_text.clone(),
            ai_summary: None,
        }
    }
}

impl PaperDetail {
    /// Attach the paper's tag and project memberships to the embedded summary.
    pub(crate) fn attach(mut self, tags: Vec<TagRef>, projects: Vec<ProjectRef>) -> Self {
        self.summary.tags = tags;
        self.summary.projects = projects;
        self
    }
}

/// PATCH /api/papers/{id}/name response: the normalized value as stored —
/// the client treats this echo as authoritative, not its own input.
#[derive(Serialize, TS)]
#[ts(export)]
struct PaperNameResponse {
    name: Option<String>,
}

/// Library counts for the header.
#[derive(Serialize, TS)]
#[ts(export)]
struct Stats {
    total: usize,
    resolved: usize,
    needs_review: usize,
}

/// Fill each row's `tags`/`projects` from its memberships — two batched
/// queries total, not two per row (`/api/papers` and `/api/search` both run
/// this over every result).
pub(super) async fn attach_row_extras(
    pool: &sqlx::SqlitePool,
    rows: &mut [PaperSummary],
) -> anyhow::Result<()> {
    let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    let mut tags = tags::for_papers(pool, &ids).await?;
    let mut projects = projects::for_papers(pool, &ids).await?;
    for r in rows.iter_mut() {
        r.tags = tags
            .remove(&r.id)
            .unwrap_or_default()
            .into_iter()
            .map(TagRef::from)
            .collect();
        r.projects = projects
            .remove(&r.id)
            .unwrap_or_default()
            .into_iter()
            .map(ProjectRef::from)
            .collect();
    }
    Ok(())
}

/// A single paper's tag and project memberships, as wire refs.
pub(super) async fn paper_refs(
    pool: &sqlx::SqlitePool,
    paper_id: &str,
) -> anyhow::Result<(Vec<TagRef>, Vec<ProjectRef>)> {
    let tags = tags::for_paper(pool, paper_id).await?;
    let projects = projects::for_paper(pool, paper_id).await?;
    Ok((
        tags.into_iter().map(TagRef::from).collect(),
        projects.into_iter().map(ProjectRef::from).collect(),
    ))
}

async fn list_papers(
    State(app): State<App>,
    Query(p): Query<ListParams>,
) -> Result<Response, ApiError> {
    let papers = papers::list(app.pool(), p.q.as_deref(), p.sort.as_deref(), &p.filter())
        .await
        .context("list_papers")?;
    let mut out: Vec<PaperSummary> = papers.iter().map(PaperSummary::from).collect();
    attach_row_extras(app.pool(), &mut out)
        .await
        .context("list_papers row extras")?;
    Ok(Json(out).into_response())
}

async fn get_paper(State(app): State<App>, Path(id): Path<String>) -> Result<Response, ApiError> {
    let p = fetch_paper(&app, &id, Trash::Allow, "get_paper").await?;
    let (tags, projects) = paper_refs(app.pool(), &p.id)
        .await
        .context("get_paper refs")?;
    let mut detail = PaperDetail::from(&p).attach(tags, projects);
    detail.ai_summary = match crate::summary::store::get(app.pool(), &p.id).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("get_paper summary for {}: {e}", p.id);
            None
        }
    };
    Ok(Json(detail).into_response())
}

/// Soft-delete a paper (web mutation): flag it deleted; the file is untouched.
/// Purge (permanent removal) is CLI-only. Idempotent on an already-trashed paper.
async fn delete_paper(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    app.trash_paper(&id)
        .await
        .map_err(ApiError::context("delete_paper"))?;
    Ok(Json(serde_json::json!({ "deleted": true })).into_response())
}

/// Un-trash a paper (the Undo behind the web UI's delete toast). 404 when the
/// paper doesn't exist or isn't trashed — `trash::restore` only flips trashed rows.
async fn restore_paper(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    app.restore_paper(&id)
        .await
        .map_err(ApiError::context("restore_paper"))?;
    Ok(Json(serde_json::json!({ "restored": true })).into_response())
}

async fn stats(State(app): State<App>) -> Result<Response, ApiError> {
    let (total, resolved, needs_review) = papers::stats(app.pool()).await.context("stats")?;
    Ok(Json(Stats {
        total: total as usize,
        resolved: resolved as usize,
        needs_review: needs_review as usize,
    })
    .into_response())
}

/// Stream a paper's PDF from the library. Range-aware (via `ServeFile`) and
/// path-safe: the resolved file must live under `library_root`.
async fn pdf(
    State(app): State<App>,
    Path(id): Path<String>,
    req: Request,
) -> Result<Response, ApiError> {
    let paper = fetch_paper(&app, &id, Trash::Allow, "pdf").await?;
    let path = library_file(app.library_root(), &paper.rel_path).await?;
    let resp = ServeFile::new(&path)
        .oneshot(req)
        .await
        .context("serve pdf")?;
    Ok(resp.map(axum::body::Body::new).into_response())
}

/// Resolve a paper's file under the library root. Defense in depth, shared by
/// every route that reads library bytes: the canonical file must live under
/// the root, and a missing one answers the same 404 a bad id would — the
/// client learns "no file here" either way.
pub(super) async fn library_file(
    library_root: &std::path::Path,
    rel_path: &str,
) -> Result<std::path::PathBuf, ApiError> {
    let path = library_root.join(rel_path);
    let under_root = {
        let (p, root) = (path.clone(), library_root.to_path_buf());
        tokio::task::spawn_blocking(move || {
            match (std::fs::canonicalize(&p), std::fs::canonicalize(&root)) {
                (Ok(file), Ok(root)) => file.starts_with(&root),
                _ => false, // missing file or unresolvable path
            }
        })
        .await
        .inspect_err(|e| tracing::error!("canonicalize check panicked: {e}"))
        .unwrap_or(false)
    };
    if under_root {
        Ok(path)
    } else {
        Err(ApiError::NotFound)
    }
}

async fn star_paper(State(app): State<App>, Path(id): Path<String>) -> Result<Response, ApiError> {
    set_star(&app, &id, true).await
}

async fn unstar_paper(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    set_star(&app, &id, false).await
}

async fn set_star(app: &App, id: &str, on: bool) -> Result<Response, ApiError> {
    if !papers::set_starred(app.pool(), id, on)
        .await
        .context("set_star")?
    {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
struct SetPaperNameBody {
    /// Missing key, null, and whitespace-only all mean "clear".
    name: Option<String>,
}

async fn set_paper_name(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(body): Json<SetPaperNameBody>,
) -> Result<Response, ApiError> {
    let name = PaperName::parse(body.name.as_deref().unwrap_or_default())?;
    if !papers::set_name(app.pool(), &id, name.as_ref())
        .await
        .context("set_paper_name")?
    {
        return Err(ApiError::NotFound);
    }
    Ok(Json(PaperNameResponse {
        name: name.map(|n| n.as_str().to_string()),
    })
    .into_response())
}

#[derive(Deserialize)]
struct FormatParam {
    format: Option<String>,
}

fn parse_format(s: Option<&str>) -> export::BibFormat {
    match s {
        Some(v) if v.eq_ignore_ascii_case("biblatex") => export::BibFormat::Biblatex,
        _ => export::BibFormat::Bibtex,
    }
}

/// One paper's `.bib` entry as plain text (inline, so the frontend can copy it
/// or force a download via `<a download>`).
async fn export_paper(
    State(app): State<App>,
    Path(id): Path<String>,
    Query(p): Query<FormatParam>,
) -> Result<Response, ApiError> {
    let paper = fetch_paper(&app, &id, Trash::Allow, "export_paper").await?;
    let body = export::format_entry(&paper, parse_format(p.format.as_deref()));
    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )],
        body,
    )
        .into_response())
}

/// The current filtered set as a downloadable `.bib` file. Same filter
/// semantics as `GET /api/papers` by construction: the query string is
/// extracted twice (`Query` reads request parts, so this composes) — once as
/// the same `ListParams` the list endpoint uses, once for `format`.
async fn export_papers(
    State(app): State<App>,
    Query(f): Query<FormatParam>,
    Query(p): Query<ListParams>,
) -> Result<Response, ApiError> {
    let papers = papers::list(app.pool(), p.q.as_deref(), p.sort.as_deref(), &p.filter())
        .await
        .context("export_papers")?;
    let body = export::format_entries(&papers, parse_format(f.format.as_deref()));
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "application/x-bibtex"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"xuewen.bib\"",
            ),
        ],
        body,
    )
        .into_response())
}
