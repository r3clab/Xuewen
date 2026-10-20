use crate::support;

use axum_test::TestServer;
use serde_json::Value;
use wiremock::matchers::{method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xuewen::app::App;
use xuewen::config::DailyConfig;
use xuewen::daily::{store, DailyService};
use xuewen::llm::LlmClient;
use xuewen::search::{embedder::Embedder, vector::QdrantStore};

fn daily_cfg() -> DailyConfig {
    DailyConfig {
        categories: vec!["cs.AI".into()],
        include_cross_list: false,
        max_papers: 20,
        run_at: "09:00".into(),
        retention_days: 14,
    }
}

/// A DailyService whose remote endpoints are all dead — fine for GET tests,
/// which never call out.
fn dead_service(pool: sqlx::SqlitePool) -> std::sync::Arc<DailyService> {
    DailyService::for_tests(
        daily_cfg(),
        pool,
        Embedder::for_tests("http://127.0.0.1:1/v1", "m", 4),
        QdrantStore::new("http://127.0.0.1:1", "xuewen", 4).unwrap(),
        LlmClient::new("http://127.0.0.1:1/v1", "m", None),
        "http://127.0.0.1:1/atom",
        "http://127.0.0.1:1/pdf",
    )
}

fn batch_paper(date: &str, rank: i64, id: &str, tldr: Option<&str>) -> store::DailyPaper {
    store::DailyPaper {
        batch_date: date.into(),
        rank,
        arxiv_id: id.into(),
        title: format!("Paper {id}"),
        authors: vec!["Ada".into()],
        abstract_text: "An abstract.".into(),
        categories: vec!["cs.AI".into()],
        score: 0.9,
        tldr: tldr.map(String::from),
        abs_url: format!("https://arxiv.org/abs/{id}"),
        pdf_url: format!("https://arxiv.org/pdf/{id}"),
        summary: None,
        code_url: None,
    }
}

#[tokio::test]
async fn get_daily_returns_latest_batch() {
    let (dir, pool) = support::temp_pool().await;
    store::replace_batch(
        &pool,
        "2026-07-09",
        &[batch_paper("2026-07-09", 1, "2507.1", None)],
    )
    .await
    .unwrap();
    let mut rich = batch_paper("2026-07-10", 1, "2507.2", Some("Short."));
    rich.summary = Some(xuewen::summary::Summary {
        tldr: "Short.".into(),
        problem: "Gap.".into(),
        approach: "Idea.".into(),
        results: "+4.2 on X.".into(),
        limitations: "Small data.".into(),
    });
    rich.code_url = Some("https://github.com/acme/widget".into());
    store::replace_batch(
        &pool,
        "2026-07-10",
        &[rich, batch_paper("2026-07-10", 2, "2507.3", None)],
    )
    .await
    .unwrap();
    let daily = dead_service(pool.clone());
    let server = TestServer::new(xuewen::web::router(
        App::builder(pool, dir.path().to_path_buf())
            .daily(daily)
            .build(),
    ))
    .unwrap();

    let resp = server.get("/api/daily").await;
    assert_eq!(resp.status_code(), 200);
    let v: Value = resp.json();
    assert_eq!(v["date"], "2026-07-10");
    assert_eq!(v["papers"].as_array().unwrap().len(), 2);
    assert_eq!(v["papers"][0]["rank"], 1);
    assert_eq!(v["papers"][0]["arxiv_id"], "2507.2");
    assert_eq!(v["papers"][0]["tldr"], "Short.");
    assert_eq!(v["papers"][0]["abstract"], "An abstract.");
    assert_eq!(v["papers"][0]["summary"]["problem"], "Gap.");
    assert_eq!(v["papers"][0]["summary"]["limitations"], "Small data.");
    assert_eq!(v["papers"][0]["code_url"], "https://github.com/acme/widget");
    assert_eq!(v["papers"][1]["tldr"], Value::Null);
    assert_eq!(v["papers"][1]["summary"], Value::Null);
    assert_eq!(v["papers"][1]["code_url"], Value::Null);
}

#[tokio::test]
async fn get_daily_empty_state_is_200_with_null_date() {
    let (dir, pool) = support::temp_pool().await;
    let daily = dead_service(pool.clone());
    let server = TestServer::new(xuewen::web::router(
        App::builder(pool, dir.path().to_path_buf())
            .daily(daily)
            .build(),
    ))
    .unwrap();
    let resp = server.get("/api/daily").await;
    assert_eq!(resp.status_code(), 200);
    let v: Value = resp.json();
    assert_eq!(v["date"], Value::Null);
    assert_eq!(v["papers"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn daily_routes_503_when_unconfigured() {
    let (dir, pool) = support::temp_pool().await;
    let server = TestServer::new(support::router(pool, dir.path().to_path_buf())).unwrap();
    assert_eq!(server.get("/api/daily").await.status_code(), 503);
    assert_eq!(server.post("/api/daily/run").await.status_code(), 503);
}

#[tokio::test]
async fn post_run_starts_then_conflicts_while_running() {
    let (dir, pool) = support::temp_pool().await;
    // Feed answers slowly so the first run stays in flight for the 409 check.
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/atom/cs.AI"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(
                    r#"<?xml version="1.0"?><feed xmlns="http://www.w3.org/2005/Atom"><title>ok</title></feed>"#,
                )
                .set_delay(std::time::Duration::from_secs(2)),
        )
        .mount(&mock)
        .await;
    let daily = DailyService::for_tests(
        daily_cfg(),
        pool.clone(),
        Embedder::for_tests("http://127.0.0.1:1/v1", "m", 4),
        QdrantStore::new("http://127.0.0.1:1", "xuewen", 4).unwrap(),
        LlmClient::new("http://127.0.0.1:1/v1", "m", None),
        &format!("{}/atom", mock.uri()),
        "http://127.0.0.1:1/pdf",
    );
    let app = xuewen::app::App::builder(pool, dir.path().to_path_buf())
        .daily(daily)
        .build();
    let tasks = app.tasks().clone();
    let server = TestServer::new(xuewen::web::router(app)).unwrap();

    assert_eq!(server.post("/api/daily/run").await.status_code(), 202);
    assert_eq!(server.post("/api/daily/run").await.status_code(), 409);
    tasks.drain().await;
}
