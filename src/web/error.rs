//! The one error type web handlers return. Every variant serializes to the
//! API's single error body shape — `{"error": "<message>"}` — so no handler
//! can invent a divergent wire format, and this is the one place a failure's
//! cause is logged (attach the operation name with `anyhow::Context` so the
//! log line still says what failed).

use axum::extract::multipart::{MultipartError, MultipartRejection};
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::ingest::import::ImportError;
use crate::library::model::NameWriteError;

pub(crate) enum ApiError {
    /// 404 with the generic "not found" body.
    NotFound,
    /// 404 with an actionable message (e.g. identify's title-search hint).
    NotFoundMsg(String),
    /// 400.
    BadRequest(String),
    /// 409; `id` names the winning paper when the conflict has one (identify).
    Conflict { message: String, id: Option<String> },
    /// 422 — a well-formed request whose content fails validation.
    Unprocessable(String),
    /// 503 — the service this endpoint needs isn't configured.
    Unavailable(String),
    /// An extractor refused the request (malformed body, bad path or query
    /// parameter, oversized upload): axum's own status and description.
    Rejected { status: StatusCode, message: String },
    /// 502 — an upstream fetch/provider call failed. `source` is logged,
    /// never sent.
    BadGateway {
        message: &'static str,
        source: Option<anyhow::Error>,
    },
    /// 500. `source` is logged, never sent.
    Internal {
        message: &'static str,
        source: anyhow::Error,
    },
}

impl ApiError {
    pub(crate) fn bad_gateway(message: &'static str, source: anyhow::Error) -> Self {
        ApiError::BadGateway {
            message,
            source: Some(source),
        }
    }

    pub(crate) fn context<C>(ctx: C) -> impl FnOnce(crate::app::error::Error) -> ApiError
    where
        C: std::fmt::Display + Send + Sync + 'static,
    {
        move |e| match e {
            crate::app::error::Error::Internal(e) => e.context(ctx).into(),
            e => e.into(),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(source: anyhow::Error) -> Self {
        ApiError::Internal {
            message: "internal error",
            source,
        }
    }
}

impl From<crate::app::error::Error> for ApiError {
    fn from(e: crate::app::error::Error) -> Self {
        use crate::app::error::Error;
        match e {
            Error::NotFound(m) => ApiError::NotFoundMsg(m),
            Error::Invalid(m) => ApiError::BadRequest(m),
            Error::Unprocessable(m) => ApiError::Unprocessable(m),
            Error::Unavailable(m) => ApiError::Unavailable(m),
            Error::LookupFailed(e) => ApiError::bad_gateway("lookup failed", e),
            Error::Import(e) => e.into(),
            Error::Internal(e) => e.into(),
        }
    }
}

impl From<ImportError> for ApiError {
    fn from(e: ImportError) -> Self {
        match e {
            ImportError::Unsupported => ApiError::BadRequest("unsupported input".into()),
            ImportError::CookieExpired => ApiError::BadGateway {
                message: "proxy session expired — refresh your cookie",
                source: None,
            },
            ImportError::Network(e) => ApiError::bad_gateway("fetch failed", e),
            ImportError::Ingest(source) => ApiError::Internal {
                message: "import failed",
                source,
            },
        }
    }
}

impl From<crate::library::model::NameError> for ApiError {
    fn from(e: crate::library::model::NameError) -> Self {
        ApiError::BadRequest(e.to_string())
    }
}

impl From<NameWriteError> for ApiError {
    fn from(e: NameWriteError) -> Self {
        match e {
            NameWriteError::Taken(_) => ApiError::Conflict {
                message: e.to_string(),
                id: None,
            },
            NameWriteError::Db(e) => anyhow::Error::new(e).into(),
        }
    }
}

macro_rules! rejected_by_axum {
    ($($rejection:ty),*) => {$(
        impl From<$rejection> for ApiError {
            fn from(r: $rejection) -> Self {
                ApiError::Rejected {
                    status: r.status(),
                    message: r.body_text(),
                }
            }
        }
    )*};
}

rejected_by_axum!(
    JsonRejection,
    PathRejection,
    QueryRejection,
    MultipartRejection,
    MultipartError
);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, json!({"error": "not found"})),
            ApiError::NotFoundMsg(m) => (StatusCode::NOT_FOUND, json!({ "error": m })),
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, json!({ "error": m })),
            ApiError::Conflict { message, id } => (
                StatusCode::CONFLICT,
                match id {
                    Some(id) => json!({"error": message, "id": id}),
                    None => json!({ "error": message }),
                },
            ),
            ApiError::Unprocessable(m) => (StatusCode::UNPROCESSABLE_ENTITY, json!({ "error": m })),
            ApiError::Unavailable(m) => (StatusCode::SERVICE_UNAVAILABLE, json!({ "error": m })),
            ApiError::Rejected { status, message } => (status, json!({ "error": message })),
            ApiError::BadGateway { message, source } => {
                if let Some(e) = source {
                    tracing::warn!("{message}: {e:#}");
                }
                (StatusCode::BAD_GATEWAY, json!({ "error": message }))
            }
            ApiError::Internal { message, source } => {
                tracing::error!("{message}: {source:#}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({ "error": message }),
                )
            }
        };
        (status, Json(body)).into_response()
    }
}
