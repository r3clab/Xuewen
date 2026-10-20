use crate::support;

use axum_test::TestServer;
use serde_json::json;
use std::path::PathBuf;
use xuewen::agent::AgentService;
use xuewen::config::{AgentBackendConfig, AgentConfig};

fn stub_agent() -> std::sync::Arc<AgentService> {
    AgentService::from_config(&AgentConfig {
        claude_code: Some(AgentBackendConfig::default()),
        codex: Some(AgentBackendConfig::default()),
        runner: Some(support::stub_runner_path()),
        ..AgentConfig::default()
    })
    .unwrap()
}

/// Router with the agent service wired: the library ships no per-service
/// constructors, so tests compose `App::builder` themselves.
fn router_with_agent(
    pool: sqlx::SqlitePool,
    root: PathBuf,
    agent: std::sync::Arc<AgentService>,
) -> axum::Router {
    let app = xuewen::app::App::builder(pool, root).agent(agent).build();
    xuewen::web::router(app)
}

#[tokio::test]
async fn models_report_unavailable_without_config() {
    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    let server = TestServer::new(support::router(pool, root)).unwrap();
    let resp = server.get("/api/chat/models").await;
    resp.assert_status_ok();
    let v: serde_json::Value = resp.json();
    assert_eq!(v["available"], false);
}

#[tokio::test]
async fn models_list_agent_backends() {
    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    let server = TestServer::new(router_with_agent(pool, root, stub_agent())).unwrap();
    let v: serde_json::Value = server.get("/api/chat/models").await.json();
    assert_eq!(v["available"], true);
    assert_eq!(v["models"][0]["id"], "claude_code");
    assert_eq!(v["models"][0]["label"], "Claude Code");
    assert_eq!(v["models"][1]["id"], "codex");
}

#[tokio::test]
async fn send_streams_tool_and_deltas_and_persists_with_tools() {
    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    let server = TestServer::new(router_with_agent(pool.clone(), root, stub_agent())).unwrap();

    let resp = server
        .post("/api/papers/p1/chat")
        .json(&json!({"model_id": "claude_code", "message": "what is this?"}))
        .await;
    resp.assert_status_ok();
    let body = resp.text();
    assert!(body.contains("event: tool"));
    assert!(body.contains("\"name\":\"Read\""));
    assert!(body.contains("event: delta"));
    assert!(body.contains("event: done"));

    let rows = xuewen::agent::history::list(&pool, "p1").await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].role, "user");
    assert_eq!(rows[1].role, "assistant");
    assert_eq!(rows[1].content, "Hello from claude_code");
    assert_eq!(rows[1].model.as_deref(), Some("Claude Code"));
    let tools: serde_json::Value =
        serde_json::from_str(rows[1].tools_json.as_deref().unwrap()).unwrap();
    assert_eq!(tools[0]["name"], "Read");
}

#[tokio::test]
async fn send_error_persists_nothing() {
    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    let server = TestServer::new(router_with_agent(pool.clone(), root, stub_agent())).unwrap();
    let resp = server
        .post("/api/papers/p1/chat")
        .json(&json!({"model_id": "codex", "message": "please fail"}))
        .await;
    resp.assert_status_ok();
    assert!(resp.text().contains("event: error"));
    assert!(xuewen::agent::history::list(&pool, "p1")
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn send_with_empty_reply_errors_and_persists_nothing() {
    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    let server = TestServer::new(router_with_agent(pool.clone(), root, stub_agent())).unwrap();
    let resp = server
        .post("/api/papers/p1/chat")
        .json(&json!({"model_id": "claude_code", "message": "answer with empty please"}))
        .await;
    resp.assert_status_ok();
    let body = resp.text();
    assert!(body.contains("event: error"));
    assert!(body.contains("empty reply"));
    assert!(xuewen::agent::history::list(&pool, "p1")
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn send_validates_model_message_paper_and_config() {
    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    let plain = TestServer::new(support::router(pool.clone(), root.clone())).unwrap();
    // Chat errors carry the API-wide `{"error": string}` body — the shape
    // the frontend's one extraction point (errorFromResponse) parses.
    let resp = plain
        .post("/api/papers/p1/chat")
        .json(&json!({"model_id": "claude_code", "message": "hi"}))
        .await;
    resp.assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        resp.json::<serde_json::Value>()["error"],
        "agent ask is not configured"
    );

    let server = TestServer::new(router_with_agent(pool.clone(), root, stub_agent())).unwrap();
    let resp = server
        .post("/api/papers/p1/chat")
        .json(&json!({"model_id": "nope", "message": "hi"}))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(
        resp.json::<serde_json::Value>()["error"],
        "unknown model_id"
    );
    server
        .post("/api/papers/p1/chat")
        .json(&json!({"model_id": "claude_code", "message": "  "}))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
    let resp = server
        .post("/api/papers/missing/chat")
        .json(&json!({"model_id": "claude_code", "message": "hi"}))
        .await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
    assert_eq!(resp.json::<serde_json::Value>()["error"], "not found");

    // Trashed paper -> 404 too: chat denies trashed papers (`Trash::Deny`).
    xuewen::library::trash::soft_delete(&pool, "p1")
        .await
        .unwrap();
    server
        .post("/api/papers/p1/chat")
        .json(&json!({"model_id": "claude_code", "message": "hi"}))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn history_roundtrip_and_clear() {
    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    xuewen::agent::history::insert_exchange(
        &pool,
        "p1",
        "q",
        "a",
        "Claude Code",
        Some(r#"[{"name":"Read","detail":"paper.txt"}]"#),
    )
    .await
    .unwrap();
    // Unparseable stored tools_json must degrade to null, not fail the load.
    xuewen::agent::history::insert_exchange(&pool, "p1", "q2", "a2", "Codex", Some("not json"))
        .await
        .unwrap();
    let server = TestServer::new(router_with_agent(pool, root, stub_agent())).unwrap();
    let rows: serde_json::Value = server.get("/api/papers/p1/chat").await.json();
    assert_eq!(rows.as_array().unwrap().len(), 4);
    // The wire carries the tool log structured (the shape the live SSE `tool`
    // events use), never the store's serialized `tools_json` TEXT.
    assert_eq!(rows[1]["tools"][0]["name"], "Read");
    assert!(rows[1].get("tools_json").is_none());
    assert!(rows[0]["tools"].is_null());
    assert!(rows[3]["tools"].is_null());
    server
        .delete("/api/papers/p1/chat")
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    let rows: serde_json::Value = server.get("/api/papers/p1/chat").await.json();
    assert_eq!(rows.as_array().unwrap().len(), 0);

    // Unknown paper: history is also guarded by the trash-denying lookup.
    server
        .get("/api/papers/nope/chat")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}
