use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};

use crate::app::App;
use crate::config::TranslateProvider;
use crate::web::error::ApiError;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/api/settings", get(get_settings))
        .route(
            "/api/settings/proxy-cookie",
            put(set_proxy_cookie).delete(clear_proxy_cookie),
        )
}

/// GET /api/settings. Reports only whether a proxy cookie is stored — never
/// the value.
#[derive(Serialize, TS)]
#[ts(export)]
struct Settings {
    /// `null` when no `[proxy]` is configured (the UI hides institutional
    /// access entirely — a cookie saved there would never be used).
    proxy: Option<ProxySettings>,
    proxy_cookie_set: bool,
    proxy_cookie_updated_at: Option<String>,
    fold_abstract: bool,
    translate: TranslateSettings,
}

/// The `proxy` block of GET /api/settings: the host of `[proxy].login_url`,
/// so the UI can name the deployment's own proxy in its cookie help copy.
#[derive(Serialize, TS)]
#[ts(export)]
struct ProxySettings {
    host: String,
}

/// The `translate` block of GET /api/settings: `{"enabled": false}` alone
/// when the service is off (the provider fields are absent, not null), the
/// full provider set when on.
#[derive(Serialize, TS)]
#[ts(export)]
struct TranslateSettings {
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    providers: Option<Vec<TranslateProvider>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    default_provider: Option<TranslateProvider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    target_lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "\"auto\" | \"manual\"")]
    trigger: Option<&'static str>,
}

impl TranslateSettings {
    fn disabled() -> Self {
        Self {
            enabled: false,
            providers: None,
            default_provider: None,
            target_lang: None,
            trigger: None,
        }
    }
}

impl From<&crate::translate::TranslateService> for TranslateSettings {
    fn from(svc: &crate::translate::TranslateService) -> Self {
        Self {
            enabled: true,
            providers: Some(svc.providers()),
            default_provider: Some(svc.default_provider()),
            target_lang: Some(svc.target_lang().to_string()),
            trigger: Some(match svc.trigger() {
                crate::config::TranslateTrigger::Auto => "auto",
                crate::config::TranslateTrigger::Manual => "manual",
            }),
        }
    }
}

#[derive(Deserialize)]
struct ProxyCookieBody {
    cookie: String,
}

/// Report whether a proxy cookie is stored (never the value itself).
async fn get_settings(State(app): State<App>) -> Result<Response, ApiError> {
    let cookie = app.proxy_cookie_status().await?;
    let translate = match app.translate().ok() {
        None => TranslateSettings::disabled(),
        Some(svc) => TranslateSettings::from(svc.as_ref()),
    };
    let proxy = app
        .ingest()
        .ok()
        .and_then(|i| i.proxy_host())
        .map(|h| ProxySettings {
            host: h.to_string(),
        });
    Ok(Json(Settings {
        proxy,
        proxy_cookie_set: cookie.set,
        proxy_cookie_updated_at: cookie.updated_at,
        fold_abstract: app.ui().fold_abstract,
        translate,
    })
    .into_response())
}

/// Store (overwrite) the EZproxy cookie.
async fn set_proxy_cookie(
    State(app): State<App>,
    Json(body): Json<ProxyCookieBody>,
) -> Result<Response, ApiError> {
    app.set_proxy_cookie(&body.cookie)
        .await
        .map_err(ApiError::context("set proxy cookie"))?;
    Ok(Json(serde_json::json!({"ok": true})).into_response())
}

/// Clear the stored EZproxy cookie.
async fn clear_proxy_cookie(State(app): State<App>) -> Result<Response, ApiError> {
    app.clear_proxy_cookie()
        .await
        .map_err(ApiError::context("clear proxy cookie"))?;
    Ok(Json(serde_json::json!({"ok": true})).into_response())
}
