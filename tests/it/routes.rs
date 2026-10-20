use std::collections::{BTreeMap, BTreeSet};

use axum::body::{to_bytes, Body};
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::Router;
use axum_test::multipart::{MultipartForm, Part};
use axum_test::TestServer;
use tower::ServiceExt;

use crate::support;

const ROUTES: &[(&str, &str)] = &[
    ("GET", "/api/papers"),
    ("POST", "/api/papers"),
    ("GET", "/api/papers/export"),
    ("GET", "/api/papers/p"),
    ("DELETE", "/api/papers/p"),
    ("POST", "/api/papers/p/restore"),
    ("GET", "/api/papers/p/export"),
    ("GET", "/api/stats"),
    ("GET", "/api/identify/search"),
    ("POST", "/api/papers/p/identify"),
    ("GET", "/papers/p/pdf"),
    ("GET", "/api/papers/p/preview"),
    ("GET", "/papers/p/preview/1"),
    ("POST", "/api/import"),
    ("GET", "/api/settings"),
    ("PUT", "/api/settings/proxy-cookie"),
    ("DELETE", "/api/settings/proxy-cookie"),
    ("GET", "/api/projects"),
    ("POST", "/api/projects"),
    ("PATCH", "/api/projects/j"),
    ("DELETE", "/api/projects/j"),
    ("PUT", "/api/papers/p/projects/j"),
    ("DELETE", "/api/papers/p/projects/j"),
    ("GET", "/api/tags"),
    ("PATCH", "/api/tags/t"),
    ("DELETE", "/api/tags/t"),
    ("PUT", "/api/papers/p/tags"),
    ("DELETE", "/api/papers/p/tags/t"),
    ("PUT", "/api/papers/p/star"),
    ("DELETE", "/api/papers/p/star"),
    ("PATCH", "/api/papers/p/name"),
    ("GET", "/api/search"),
    ("GET", "/api/search/status"),
    ("GET", "/api/daily"),
    ("POST", "/api/daily/run"),
    ("GET", "/api/chat/models"),
    ("GET", "/api/papers/p/chat"),
    ("POST", "/api/papers/p/chat"),
    ("DELETE", "/api/papers/p/chat"),
    ("POST", "/api/papers/p/citations"),
    ("GET", "/api/papers/p/annotations"),
    ("DELETE", "/api/papers/p/annotations"),
    ("PUT", "/api/papers/p/annotations/a"),
    ("DELETE", "/api/papers/p/annotations/a"),
    ("GET", "/api/papers/p/code"),
    ("PUT", "/api/papers/p/code"),
    ("DELETE", "/api/papers/p/code"),
    ("POST", "/api/translate"),
];

async fn base_router() -> (tempfile::TempDir, Router) {
    let (dir, pool) = support::temp_pool().await;
    let router = support::router(pool, dir.path().join("library"));
    (dir, router)
}

async fn send(router: &Router, method: &str, path: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
    let req = Request::builder()
        .method(Method::from_bytes(method.as_bytes()).unwrap())
        .uri(path)
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let (parts, body) = resp.into_parts();
    let bytes = to_bytes(body, usize::MAX).await.unwrap();
    (parts.status, parts.headers, bytes.to_vec())
}

/// Whether a handler answered: the static fallback serves the SPA shell
/// (`text/html`) or a bare-text 404, and a method miss on a routed path is a
/// 405, while every handler 404 carries the API's `{"error": ...}` body.
async fn routed(router: &Router, method: &str, path: &str) -> Result<(), String> {
    let (status, headers, body) = send(router, method, path).await;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if status == StatusCode::METHOD_NOT_ALLOWED {
        return Err(format!("{method} {path}: 405"));
    }
    if content_type.starts_with("text/html") {
        return Err(format!("{method} {path}: static fallback ({status})"));
    }
    if status == StatusCode::NOT_FOUND {
        let error = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("error").cloned());
        if error.is_none() {
            return Err(format!("{method} {path}: unrouted 404"));
        }
    }
    Ok(())
}

#[tokio::test]
async fn every_route_reaches_a_handler() {
    let (_dir, router) = base_router().await;
    let mut failures = Vec::new();
    for (method, path) in ROUTES {
        if let Err(e) = routed(&router, method, path).await {
            failures.push(e);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[tokio::test]
async fn each_path_allows_exactly_its_methods() {
    let (_dir, router) = base_router().await;
    let mut expected: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for (method, path) in ROUTES {
        let methods = expected.entry(path).or_default();
        methods.insert(method.to_string());
        if *method == "GET" {
            methods.insert("HEAD".into());
        }
    }
    for (path, methods) in expected {
        let (status, headers, _) = send(&router, "OPTIONS", path).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "OPTIONS {path}");
        let allow: BTreeSet<String> = headers
            .get(header::ALLOW)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(',')
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .collect();
        assert_eq!(allow, methods, "Allow for {path}");
    }
}

#[tokio::test]
async fn unrouted_requests_fail_the_check() {
    let (_dir, router) = base_router().await;
    assert!(routed(&router, "GET", "/api/no-such-route").await.is_err());
    assert!(routed(&router, "PATCH", "/api/stats").await.is_err());
}

#[tokio::test]
async fn upload_route_accepts_bodies_over_the_default_limit() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = xuewen::db::connect(&url).await.unwrap();
    let ingest = support::ingest(
        pool.clone(),
        dir.path(),
        support::offline_resolver(),
        xuewen::ingest::fetch::Fetcher::new(None).unwrap(),
    );
    let app = xuewen::app::App::builder(pool, library)
        .ingest(ingest)
        .build();
    let server = TestServer::new(xuewen::web::router(app)).unwrap();

    // Over axum's 2 MiB default: the upload limit must let it through to the
    // handler, which then rejects the bytes as not a PDF.
    let form = MultipartForm::new().add_part(
        "file",
        Part::bytes(vec![b'x'; 3 * 1024 * 1024]).file_name("big.pdf"),
    );
    let resp = server.post("/api/papers").multipart(form).await;
    resp.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(resp.json::<serde_json::Value>()["error"], "not a PDF");
}
