use crate::support;

use axum_test::TestServer;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xuewen::config::Config;
use xuewen::translate::TranslateService;

#[tokio::test]
async fn translate_returns_503_when_disabled() {
    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    let server = TestServer::new(support::router(pool, root)).unwrap();
    server
        .post("/api/translate")
        .json(&serde_json::json!({"text": "hello"}))
        .await
        .assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn settings_report_translate_disabled_by_default() {
    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    let server = TestServer::new(support::router(pool, root)).unwrap();
    let resp = server.get("/api/settings").await;
    resp.assert_status_ok();
    let v: serde_json::Value = resp.json();
    assert_eq!(v["translate"]["enabled"], false);
}

fn translate_cfg(base_url: &str) -> Config {
    let toml = format!(
        "inbox_dir='/i'\nlibrary_root='/l'\ndatabase_url='sqlite::memory:'\n\
         [ai]\napi_key='k'\nbase_url='{base_url}'\n\
         [ai.translate]\nmodel='mock-1'\n"
    );
    toml::from_str(&toml).unwrap()
}

#[tokio::test]
async fn translate_returns_translation_from_mocked_llm() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": "你好"}}]
        })))
        .mount(&upstream)
        .await;

    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    let cfg = translate_cfg(&upstream.uri());
    let translate = std::sync::Arc::new(TranslateService::from_config(&cfg).unwrap());
    let server = {
        let app = xuewen::app::App::builder(pool, root)
            .translate(translate)
            .build();
        TestServer::new(xuewen::web::router(app)).unwrap()
    };

    let resp = server
        .post("/api/translate")
        .json(&json!({"text": "hello"}))
        .await;
    resp.assert_status_ok();
    let v: serde_json::Value = resp.json();
    assert_eq!(v["translation"], "你好");
    assert_eq!(v["provider"], "llm");
}
