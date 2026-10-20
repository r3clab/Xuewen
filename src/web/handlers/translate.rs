use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};

use crate::app::App;
use crate::config::TranslateProvider;
use crate::web::error::ApiError;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub(super) fn routes() -> Router<App> {
    Router::new().route("/api/translate", post(translate))
}

/// POST /api/translate response.
#[derive(Serialize, TS)]
#[ts(export)]
struct TranslationDto {
    translation: String,
    provider: TranslateProvider,
    source_lang: Option<String>,
    target_lang: String,
}

impl From<crate::translate::Translated> for TranslationDto {
    fn from(t: crate::translate::Translated) -> Self {
        Self {
            translation: t.text,
            provider: t.provider,
            source_lang: t.source_lang,
            target_lang: t.target_lang,
        }
    }
}

#[derive(Deserialize)]
struct TranslateBody {
    text: String,
    #[serde(default)]
    target_lang: Option<String>,
    #[serde(default)]
    provider: Option<String>,
}

/// Translate arbitrary text via the configured provider. 503 when
/// translate-on-selection isn't configured; 400 on empty text or an unknown
/// provider name; 502 when the upstream provider call fails.
async fn translate(
    State(app): State<App>,
    Json(body): Json<TranslateBody>,
) -> Result<Response, ApiError> {
    let svc = app.translate()?;
    let text = body.text.trim();
    if text.is_empty() {
        return Err(ApiError::BadRequest("empty text".into()));
    }
    let provider = match body.provider.as_deref() {
        None => None,
        Some("llm") => Some(crate::config::TranslateProvider::Llm),
        Some("deepl") => Some(crate::config::TranslateProvider::Deepl),
        Some(other) => return Err(ApiError::BadRequest(format!("unknown provider `{other}`"))),
    };
    match svc
        .translate(text, body.target_lang.as_deref(), provider)
        .await
    {
        Ok(t) => Ok(Json(TranslationDto::from(t)).into_response()),
        Err(e) => {
            tracing::error!("translate: {e}");
            Err(ApiError::BadGateway("translation failed"))
        }
    }
}
