use crate::support;

use axum_test::TestServer;
use serde_json::json;
use xuewen::agent::AgentService;
use xuewen::config::{AgentBackendConfig, AgentConfig};

fn stub_agent() -> std::sync::Arc<AgentService> {
    AgentService::from_config(&AgentConfig {
        claude_code: Some(AgentBackendConfig::default()),
        runner: Some(support::stub_runner_path()),
        ..AgentConfig::default()
    })
    .unwrap()
}

#[tokio::test]
async fn code_endpoints_gate_validate_and_report() {
    let (pool, root) = support::pool_and_root_with_paper("p1").await;

    // 503 without the agent service.
    let plain = TestServer::new(support::router(pool.clone(), root.clone())).unwrap();
    plain
        .put("/api/papers/p1/code")
        .json(&json!({"repo_url": "https://github.com/x/y"}))
        .await
        .assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);

    let app = xuewen::app::App::builder(pool.clone(), root)
        .agent(stub_agent())
        // `invalid.example` (RFC 2606) is allowlisted so the accept-then-fail
        // path is reachable; it never resolves, so the clone fails offline.
        .clone_allowed_hosts(vec!["invalid.example".to_string()])
        .build();
    let tasks = app.tasks().clone();
    let server = TestServer::new(xuewen::web::router(app)).unwrap();

    // Nothing attached yet.
    let v: serde_json::Value = server.get("/api/papers/p1/code").await.json();
    assert_eq!(v["attached"], false);

    // Bad URLs are rejected up front with JSON error body.
    let bad_url_resp = server
        .put("/api/papers/p1/code")
        .json(&json!({"repo_url": "git@github.com:x/y.git"}))
        .await;
    bad_url_resp.assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    let bad_url_json: serde_json::Value = bad_url_resp.json();
    assert!(bad_url_json.get("error").is_some());
    assert!(
        bad_url_json["error"]
            .as_str()
            .map(|s| s.contains("https"))
            .unwrap_or(false)
            || bad_url_json["error"]
                .as_str()
                .map(|s| s.contains("credentials"))
                .unwrap_or(false),
        "error message should mention https or credentials, got: {:?}",
        bad_url_json["error"]
    );

    // A valid URL is accepted and the row enters 'cloning' (the background
    // clone then fails offline, flipping it to 'error' — wait for the job).
    let resp = server
        .put("/api/papers/p1/code")
        .json(&json!({"repo_url": "https://invalid.example/none.git"}))
        .await;
    resp.assert_status(axum::http::StatusCode::ACCEPTED);
    tasks.drain().await;
    let v: serde_json::Value = server.get("/api/papers/p1/code").await.json();
    assert_eq!(v["code"]["status"], "error");

    // Detach removes row and checkout.
    server
        .delete("/api/papers/p1/code")
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    let v: serde_json::Value = server.get("/api/papers/p1/code").await.json();
    assert_eq!(v["attached"], false);

    // Unknown paper -> 404.
    server
        .get("/api/papers/missing/code")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // Unknown paper PUT -> 404.
    server
        .put("/api/papers/missing/code")
        .json(&json!({"repo_url": "https://github.com/x/y"}))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // Unknown paper DELETE -> 404.
    server
        .delete("/api/papers/missing/code")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // Trashed paper -> 404: the code endpoints deny trashed papers
    // (`Trash::Deny` in web/handlers/code.rs).
    xuewen::library::trash::soft_delete(&pool, "p1")
        .await
        .unwrap();
    server
        .get("/api/papers/p1/code")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

/// Regression for the whole-branch-review purge bug: `paper_code` had no
/// `ON DELETE CASCADE` on its `paper_id` FK, so purging a paper with an
/// attached repo hit `FOREIGN KEY constraint failed` after the PDF was
/// already removed. This drives `App::purge` — the sequence
/// `xuewen purge` itself runs — and asserts everything is gone afterward.
/// Its explicit sidecar deletes are belt-and-braces beside the migrations'
/// cascades, which is why the cascade gets its own test below: the explicit
/// `delete_paper_code` call would mask a cascade regression on its own.
#[tokio::test]
async fn purge_clears_paper_code_and_agent_workspace() {
    let (pool, root) = support::pool_and_root_with_paper("p1").await;

    xuewen::agent::store::upsert_paper_code_cloning(&pool, "p1", "https://github.com/x/y")
        .await
        .unwrap();
    assert!(xuewen::agent::store::get_paper_code(&pool, "p1")
        .await
        .unwrap()
        .is_some());

    let ws = xuewen::agent::workspace_dir(&root, "p1");
    let repo_dir = ws.join("repo");
    std::fs::create_dir_all(&repo_dir).unwrap();
    std::fs::write(repo_dir.join("dummy.txt"), b"hello").unwrap();
    assert!(ws.exists());

    let paper = xuewen::library::papers::get(&pool, "p1")
        .await
        .unwrap()
        .unwrap();
    let cache = root.join("preview-cache");
    let cached_pages = xuewen::preview::cache::paper_dir(&cache, &paper.content_hash);
    std::fs::create_dir_all(&cached_pages).unwrap();
    std::fs::write(cached_pages.join("0.png"), b"png").unwrap();

    xuewen::app::App::builder(pool.clone(), root.clone())
        .build()
        .purge(&paper)
        .await
        .unwrap();

    assert!(xuewen::agent::store::get_paper_code(&pool, "p1")
        .await
        .unwrap()
        .is_none());
    assert!(!ws.exists());
    assert!(
        !cached_pages.exists(),
        "rendered previews go with the paper"
    );
    let row: Option<(String,)> = sqlx::query_as("SELECT id FROM papers WHERE id = ?")
        .bind("p1")
        .fetch_optional(&pool)
        .await
        .unwrap();
    assert!(row.is_none());
}

/// Isolates the migration's `ON DELETE CASCADE` itself: deletes the papers
/// row directly (no explicit `delete_paper_code` first) and asserts the FK
/// cascade alone removes the attached `paper_code` row, rather than failing
/// with `FOREIGN KEY constraint failed`.
#[tokio::test]
async fn deleting_paper_cascades_to_paper_code() {
    let (pool, _root) = support::pool_and_root_with_paper("p1").await;

    xuewen::agent::store::upsert_paper_code_cloning(&pool, "p1", "https://github.com/x/y")
        .await
        .unwrap();
    assert!(xuewen::agent::store::get_paper_code(&pool, "p1")
        .await
        .unwrap()
        .is_some());

    sqlx::query("DELETE FROM papers WHERE id = ?")
        .bind("p1")
        .execute(&pool)
        .await
        .expect("delete should succeed under the ON DELETE CASCADE fix");

    assert!(xuewen::agent::store::get_paper_code(&pool, "p1")
        .await
        .unwrap()
        .is_none());
}
