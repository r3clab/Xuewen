use std::collections::{BTreeMap, BTreeSet};

use axum::body::{to_bytes, Body};
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::Router;
use axum_test::multipart::{MultipartForm, Part};
use axum_test::TestServer;
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::support;

const ROUTES: &[(&str, &str)] = &[
    ("GET", "/api/health"),
    ("GET", "/api/papers"),
    ("POST", "/api/papers"),
    ("POST", "/api/papers/export"),
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
    ("POST", "/api/citations/match"),
    ("GET", "/api/papers/p/annotations"),
    ("DELETE", "/api/papers/p/annotations"),
    ("PUT", "/api/papers/p/annotations/a"),
    ("DELETE", "/api/papers/p/annotations/a"),
    ("GET", "/api/papers/p/code"),
    ("PUT", "/api/papers/p/code"),
    ("DELETE", "/api/papers/p/code"),
    ("POST", "/api/translate"),
];

const UNROUTED: &str = "no such endpoint";

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

/// Whether a handler answered: a method miss on a routed path is a 405, and
/// the fallback answers an API path with the "no such endpoint" 404 (any
/// other path with the SPA shell), while a handler's own 404 names what is
/// missing.
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
        match error {
            Some(e) if e != UNROUTED => {}
            _ => return Err(format!("{method} {path}: unrouted 404")),
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
async fn health_answers_without_the_database() {
    let (dir, pool) = support::temp_pool().await;
    let router = support::router(pool.clone(), dir.path().join("library"));
    pool.close().await;
    let (status, _, body) = send(&router, "GET", "/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({"status": "ok"})
    );
}

#[tokio::test]
async fn unknown_api_paths_answer_json_and_the_rest_get_the_spa() {
    let (_dir, router) = base_router().await;
    for path in [
        "/api",
        "/api/paperz",
        "/api/papers/p/nope",
        "/papers/p/nope",
    ] {
        let (status, headers, body) = send(&router, "GET", path).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(headers[header::CONTENT_TYPE], "application/json", "{path}");
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["error"], UNROUTED, "{path}");
    }
    for path in ["/", "/library", "/apiary"] {
        let (status, headers, _) = send(&router, "GET", path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        let content_type = headers[header::CONTENT_TYPE].to_str().unwrap();
        assert!(
            content_type.starts_with("text/html"),
            "{path}: {content_type}"
        );
    }
}

#[tokio::test]
async fn malformed_requests_answer_in_the_api_error_shape() {
    let (_dir, router) = base_router().await;
    let json_post = |body: &'static str| {
        Request::post("/api/projects")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap()
    };
    let get = |uri: &str| Request::get(uri).body(Body::empty()).unwrap();
    let cases = [
        (json_post("{"), StatusCode::BAD_REQUEST),
        (
            json_post(r#"{"name": 1}"#),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            Request::post("/api/projects")
                .body(Body::from(r#"{"name": "x"}"#))
                .unwrap(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (get("/papers/p/preview/first"), StatusCode::BAD_REQUEST),
        (
            get("/api/papers/p/export?format=ris"),
            StatusCode::BAD_REQUEST,
        ),
    ];
    for (req, expected) in cases {
        let what = format!("{} {}", req.method(), req.uri());
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), expected, "{what}");
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert!(body["error"].is_string(), "{what}: {body}");
    }
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
        xuewen::metadata::resolve::Resolver::offline(),
        xuewen::ingest::fetch::Fetcher::new(None).unwrap(),
    );
    let app = xuewen::app::App::builder(pool, library)
        .ingest(ingest)
        .build();
    let server = TestServer::new(xuewen::web::router(app));

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
