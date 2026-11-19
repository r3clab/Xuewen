use anyhow::Context as _;
use axum::extract::multipart::MultipartRejection;
use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::routing::post;
use axum::{Json, Router};

use crate::app::App;
use crate::ingest::MAX_PDF_BYTES;
use crate::ingest::import::Imported;
use crate::ingest::pipeline::Outcome;
use crate::library::model::PaperStatus;
use crate::library::papers;
use crate::web::error::ApiError;
use crate::web::extract::ApiJson;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route(
            "/api/papers",
            post(import_paper).layer(DefaultBodyLimit::max(MAX_PDF_BYTES)),
        )
        .route("/api/import", post(import_url))
}

/// Outcome of POST /api/papers (upload) and POST /api/import (URL), tagged on
/// `outcome`. `Unfetched` is URL-import only: metadata resolved but no PDF
/// could be fetched.
#[derive(Serialize, TS)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[ts(export, rename = "ImportResult")]
enum ImportOutcome {
    Ingested {
        id: String,
        title: Option<String>,
        status: PaperStatus,
    },
    Duplicate {
        id: String,
    },
    SameWork {
        id: String,
    },
    InTrash {
        id: String,
    },
    Unfetched {
        title: Option<String>,
        doi: Option<String>,
    },
}

/// Import a single uploaded PDF: validate, stage into `inbox_dir/_uploads`, and
/// run the ingest pipeline. One PDF per request (the frontend uploads files one
/// at a time). Returns `ingested` (with title/status), `duplicate`, or an error.
async fn import_paper(
    State(app): State<App>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<Json<ImportOutcome>, ApiError> {
    app.ingest()?;
    let mut multipart = multipart?;
    // Take the first file part; skip any non-file fields.
    while let Some(field) = multipart.next_field().await? {
        let Some(filename) = field.file_name().map(str::to_string) else {
            continue;
        };
        let data = field.bytes().await?;
        if !data.starts_with(b"%PDF") {
            return Err(ApiError::BadRequest("not a PDF".into()));
        }
        let outcome = app.ingest_upload(&filename, &data).await?;
        return import_outcome(app.pool(), outcome).await;
    }
    Err(ApiError::BadRequest("no file".into()))
}

/// The wire form of an ingest outcome, shared by file upload and URL import:
/// `Ingested` carries the new paper's title and status.
async fn import_outcome(
    pool: &sqlx::SqlitePool,
    outcome: Outcome,
) -> Result<Json<ImportOutcome>, ApiError> {
    Ok(Json(match outcome {
        Outcome::Ingested(id) => {
            let paper = papers::get(pool, &id)
                .await
                .context("imported paper lookup")?
                .ok_or(ApiError::NotFound)?;
            ImportOutcome::Ingested {
                id,
                title: paper.meta.title,
                status: paper.meta.status,
            }
        }
        Outcome::Duplicate(id) => ImportOutcome::Duplicate { id },
        Outcome::SameWork(id) => ImportOutcome::SameWork { id },
        Outcome::InTrash(id) => ImportOutcome::InTrash { id },
    }))
}

#[derive(Deserialize, TS)]
#[ts(export)]
struct ImportUrlBody {
    input: String,
}

/// Import from a URL/DOI/arXiv id: fetch the PDF (arXiv/proxy/OA), then ingest.
async fn import_url(
    State(app): State<App>,
    ApiJson(body): ApiJson<ImportUrlBody>,
) -> Result<Json<ImportOutcome>, ApiError> {
    match app.import(&body.input).await? {
        Imported::Ingested(outcome) => import_outcome(app.pool(), outcome).await,
        Imported::Unfetched { title, doi } => Ok(Json(ImportOutcome::Unfetched { title, doi })),
    }
}
