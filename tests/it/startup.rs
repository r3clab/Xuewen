use std::time::Duration;

use xuewen::app::App;
use xuewen::search::vector::{OpenError, OpenMode, VectorStore};

use crate::support;

#[tokio::test]
async fn app_start_with_minimal_config() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = support::test_cfg(tmp.path());
    let pool = xuewen::db::connect(&cfg.database_url).await.unwrap();

    let app = App::start(&cfg, pool).await.unwrap();

    // No [ai.*]/[daily]/[translate] sections => those services are off;
    // ingest and citations are always built.
    assert!(app.agent().is_err());
    assert!(app.daily().is_err());
    assert!(app.translate().is_err());
    assert_eq!(
        app.ingest().unwrap().staging_dir(),
        cfg.inbox_dir.join("_uploads")
    );
}

#[tokio::test]
async fn started_app_serves_the_full_stack() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = support::test_cfg(tmp.path());
    let pool = xuewen::db::connect(&cfg.database_url).await.unwrap();
    let app = App::start(&cfg, pool.clone()).await.unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(xuewen::web::serve_on(
        listener,
        app.clone(),
        app.tasks().token(),
    ));

    let resp = reqwest::get(format!("http://{addr}/api/stats"))
        .await
        .unwrap();
    assert!(resp.status().is_success(), "got {}", resp.status());

    tokio::time::timeout(Duration::from_secs(5), app.shutdown())
        .await
        .expect("background workers stop on shutdown");
    assert!(pool.is_closed());
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("server stops on shutdown")
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn failed_start_leaves_no_background_work_running() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = support::test_cfg_with(tmp.path(), "\n[daily]\ncategories = []\n");
    let pool = xuewen::db::connect(&cfg.database_url).await.unwrap();
    let metrics = tokio::runtime::Handle::current().metrics();
    let before = metrics.num_alive_tasks();

    let err = App::start(&cfg, pool)
        .await
        .err()
        .expect("empty [daily].categories fails startup");

    assert!(err.to_string().contains("[daily].categories"), "{err:#}");
    assert_eq!(metrics.num_alive_tasks(), before);
}

#[tokio::test]
async fn serve_on_serves_api_on_an_ephemeral_port() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = support::test_cfg(tmp.path());
    let pool = xuewen::db::connect(&cfg.database_url).await.unwrap();

    let app = App::builder(pool.clone(), cfg.library_root.clone())
        .citations(xuewen::citations::CitationsService::from_config(pool, &cfg))
        .build();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    assert_ne!(addr.port(), 0, "caller can learn the real bound port");

    let token = tokio_util::sync::CancellationToken::new();
    let server = tokio::spawn(xuewen::web::serve_on(listener, app, token.clone()));

    let resp = reqwest::get(format!("http://{addr}/api/stats"))
        .await
        .unwrap();
    assert!(resp.status().is_success(), "got {}", resp.status());

    token.cancel();
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("server stops once cancelled")
        .unwrap()
        .unwrap();
}

/// Embedding and daily both on, against an endpoint nothing listens on: the
/// app never calls it during these tests.
const SEMANTIC_AND_DAILY: &str = r#"
[daily]
categories = ["cs.AI"]

[ai]
base_url = "http://127.0.0.1:1/v1"
api_key = "k"
model = "m"

[ai.embedding]
model = "m1"
dims = 4

[ai.daily]
"#;

#[tokio::test]
async fn started_app_shares_one_vector_store_and_releases_it_on_shutdown() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = support::test_cfg_with(tmp.path(), SEMANTIC_AND_DAILY);
    let pool = xuewen::db::connect(&cfg.database_url).await.unwrap();
    let app = App::start(&cfg, pool).await.unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(xuewen::web::serve_on(
        listener,
        app.clone(),
        app.tasks().token(),
    ));

    assert!(app.daily().is_ok(), "daily got the store");
    let status: serde_json::Value = reqwest::get(format!("http://{addr}/api/search/status"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        status["reason"],
        serde_json::Value::Null,
        "search got the store too"
    );

    let contender = VectorStore::open(&cfg.search.vector_dir, 4, OpenMode::LoadOrCreate).await;
    match contender {
        Err(OpenError::Locked { holder, .. }) => assert_eq!(holder, Some(addr)),
        other => panic!("expected Locked, got {:?}", other.map(|(_, o)| o)),
    }

    tokio::time::timeout(Duration::from_secs(5), app.shutdown())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    VectorStore::open(&cfg.search.vector_dir, 4, OpenMode::LoadOrCreate)
        .await
        .expect("shutdown released the store");
}
