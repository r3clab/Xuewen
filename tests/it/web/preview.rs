use crate::support;

use axum::http::StatusCode;
use axum_test::TestServer;

/// Seed a paper whose `rel_path` points at real bytes under the library root.
async fn server_with_pdf(bytes: &[u8]) -> TestServer {
    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    std::fs::write(root.join("missing.pdf"), bytes).unwrap();
    TestServer::new(support::router(pool, root)).unwrap()
}

fn real_pdf() -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.pdf");
    support::write_test_pdf(&path, &["Preview fixture"]);
    std::fs::read(path).unwrap()
}

#[tokio::test]
async fn reports_page_count_and_geometry() {
    let server = server_with_pdf(&real_pdf()).await;
    let v: serde_json::Value = server.get("/api/papers/p1/preview").await.json();
    assert_eq!(v["pages"], 1);
    // A4 portrait: taller than it is wide, in PDF points.
    assert!(v["page_width"].as_f64().unwrap() > 0.0);
    assert!(v["page_height"].as_f64().unwrap() > v["page_width"].as_f64().unwrap());
}

#[tokio::test]
async fn serves_a_page_as_png() {
    let server = server_with_pdf(&real_pdf()).await;
    let resp = server.get("/papers/p1/preview/0").await;
    resp.assert_status_ok();
    assert_eq!(resp.header("content-type"), "image/png");
    assert_eq!(&resp.as_bytes()[..8], b"\x89PNG\r\n\x1a\n");
}

/// A rendered page is written to the cache and the next request is served
/// from it. The file is keyed by the paper's `content_hash`, which the
/// seeding helper sets to the id.
#[tokio::test]
async fn a_rendered_page_is_cached_on_disk() {
    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    std::fs::write(root.join("missing.pdf"), real_pdf()).unwrap();
    let cached = root.join("preview-cache").join("p1").join("0.png");
    let server = TestServer::new(support::router(pool, root)).unwrap();

    let first = server.get("/papers/p1/preview/0").await.as_bytes().to_vec();
    assert_eq!(std::fs::read(&cached).unwrap(), first);

    // Overwriting the cache entry proves the second request reads it rather
    // than rendering again — a re-render could not produce these bytes.
    std::fs::write(&cached, b"cache-hit").unwrap();
    assert_eq!(
        server.get("/papers/p1/preview/0").await.as_bytes().to_vec(),
        b"cache-hit".to_vec()
    );
}

#[tokio::test]
async fn a_page_past_the_end_is_not_found() {
    let server = server_with_pdf(&real_pdf()).await;
    server
        .get("/papers/p1/preview/7")
        .await
        .assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_non_numeric_page_is_a_bad_request_in_the_usual_shape() {
    let server = server_with_pdf(&real_pdf()).await;
    let resp = server.get("/papers/p1/preview/first").await;
    resp.assert_status(StatusCode::BAD_REQUEST);
    // Every error this API returns is `{"error": "..."}`, including this one
    // — which is why the page number is parsed by hand.
    let v: serde_json::Value = resp.json();
    assert!(v["error"].is_string());
}

/// An unreadable PDF is 422, not 500: nothing is broken, this paper simply
/// has no image to show, and 422 is what tells the picker to draw its text
/// card instead.
#[tokio::test]
async fn an_unrenderable_pdf_is_unprocessable() {
    let server = server_with_pdf(b"this is not a PDF").await;
    server
        .get("/api/papers/p1/preview")
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    server
        .get("/papers/p1/preview/0")
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY);
}

/// Page geometry is memoized per service, not per process. Every test here
/// seeds the same paper id as its content hash, so a process-wide memo would
/// let one server's real A4 answer stand in for another server's garbage.
#[tokio::test]
async fn page_meta_is_not_shared_across_services() {
    let real = server_with_pdf(&real_pdf()).await;
    real.get("/api/papers/p1/preview").await.assert_status_ok();
    let garbage = server_with_pdf(b"this is not a PDF").await;
    garbage
        .get("/api/papers/p1/preview")
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn an_unknown_paper_is_not_found() {
    let server = server_with_pdf(&real_pdf()).await;
    server
        .get("/api/papers/nope/preview")
        .await
        .assert_status(StatusCode::NOT_FOUND);
    server
        .get("/papers/nope/preview/0")
        .await
        .assert_status(StatusCode::NOT_FOUND);
}

/// The stock seeded paper points at a file that was never written.
#[tokio::test]
async fn a_missing_library_file_is_not_found() {
    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    let server = TestServer::new(support::router(pool, root)).unwrap();
    server
        .get("/api/papers/p1/preview")
        .await
        .assert_status(StatusCode::NOT_FOUND);
}
