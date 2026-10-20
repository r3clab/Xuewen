use axum::extract::multipart::MultipartError;
use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};

use crate::app::error::Error;
use crate::app::App;
use crate::ingest::import::{ImportError, Imported};
use crate::ingest::pipeline::Outcome;
use crate::library::model::PaperStatus;
use crate::library::papers;
use crate::web::error::ApiError;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route(
            "/api/papers",
            post(import_paper).layer(DefaultBodyLimit::max(50 * 1024 * 1024)),
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
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    app.ingest()?;

    // Take the first file part; skip any non-file fields.
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => return Err(ApiError::BadRequest("no file".into())),
            Err(e) => return Ok(multipart_error(e)),
        };
        let Some(filename) = field.file_name().map(|s| s.to_string()) else {
            continue;
        };
        let data = match field.bytes().await {
            Ok(b) => b,
            Err(e) => return Ok(multipart_error(e)),
        };
        if !data.starts_with(b"%PDF") {
            return Err(ApiError::BadRequest("not a PDF".into()));
        }

        let outcome = match app.ingest_upload(&filename, &data).await {
            Ok(outcome) => Ok(outcome),
            Err(Error::Internal(e)) => Err(e),
            Err(e) => return Err(e.into()),
        };
        return Ok(ingest_response(app.pool(), outcome).await);
    }
}

/// Map an ingest result to the shared `ImportOutcome` JSON (`Ingested`
/// enriched with title/status), or a failed ingest to the "import failed" 500
/// — not the generic "internal error": "import failed" is the body the
/// frontend has always surfaced. Shared by file upload and URL import.
async fn ingest_response(pool: &sqlx::SqlitePool, result: anyhow::Result<Outcome>) -> Response {
    match result {
        Ok(Outcome::Ingested(id)) => {
            let (title, status) = match papers::get(pool, &id).await {
                Ok(Some(p)) => (p.meta.title, p.meta.status),
                _ => (None, crate::library::model::PaperStatus::NeedsReview),
            };
            Json(ImportOutcome::Ingested { id, title, status }).into_response()
        }
        Ok(Outcome::Duplicate(id)) => Json(ImportOutcome::Duplicate { id }).into_response(),
        Ok(Outcome::SameWork(id)) => Json(ImportOutcome::SameWork { id }).into_response(),
        Ok(Outcome::InTrash(id)) => Json(ImportOutcome::InTrash { id }).into_response(),
        Err(e) => {
            tracing::error!("import ingest: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "import failed"})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct ImportUrlBody {
    input: String,
}

/// Import from a URL/DOI/arXiv id: fetch the PDF (arXiv/proxy/OA), then ingest.
async fn import_url(
    State(app): State<App>,
    Json(body): Json<ImportUrlBody>,
) -> Result<Response, ApiError> {
    let outcome = match app.import(&body.input).await {
        Ok(Imported::Ingested(outcome)) => Ok(outcome),
        Ok(Imported::Unfetched { title, doi }) => {
            return Ok(Json(ImportOutcome::Unfetched { title, doi }).into_response());
        }
        Err(ImportError::Ingest(e)) => Err(e),
        Err(ImportError::Unavailable(m)) => return Err(ApiError::Unavailable(m)),
        Err(ImportError::Internal(e)) => {
            return Err(ApiError::Internal(e.context("import proxy cookie")))
        }
        Err(ImportError::Unsupported) => {
            return Err(ApiError::BadRequest("unsupported input".into()))
        }
        Err(ImportError::CookieExpired) => {
            return Err(ApiError::BadGateway(
                "proxy session expired — refresh your cookie",
            ))
        }
        Err(ImportError::Network(e)) => {
            tracing::error!("import fetch: {e}");
            return Err(ApiError::BadGateway("fetch failed"));
        }
    };
    Ok(ingest_response(app.pool(), outcome).await)
}

/// Map a multipart read error to its proper status (e.g. 413 when the body
/// exceeds the limit) with a JSON body.
fn multipart_error(e: MultipartError) -> Response {
    let status = e.status();
    (
        status,
        Json(serde_json::json!({
            "error": status.canonical_reason().unwrap_or("upload error").to_lowercase()
        })),
    )
        .into_response()
}
