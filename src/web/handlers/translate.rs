use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};

use crate::app::App;
use crate::config::TranslateProvider;
use crate::translate::TranslateError;
use crate::web::error::ApiError;
use crate::web::extract::ApiJson;
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

#[derive(Deserialize, TS)]
#[ts(export)]
struct TranslateBody {
    text: String,
    #[serde(default)]
    #[ts(optional)]
    target_lang: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    provider: Option<TranslateProvider>,
}

/// Translate arbitrary text via the configured provider. 503 when
/// translate-on-selection isn't configured; 400 on empty text; 502 when the upstream provider call fails.
async fn translate(
    State(app): State<App>,
    ApiJson(body): ApiJson<TranslateBody>,
) -> Result<Response, ApiError> {
    let svc = app.translate()?;
    match svc
        .translate(&body.text, body.target_lang.as_deref(), body.provider)
        .await
    {
        Ok(t) => Ok(Json(TranslationDto::from(t)).into_response()),
        Err(TranslateError::Empty) => Err(ApiError::BadRequest("empty text".into())),
        Err(TranslateError::Failed(e)) => Err(ApiError::bad_gateway("translation failed", e)),
    }
}
