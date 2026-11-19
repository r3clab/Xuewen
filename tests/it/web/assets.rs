//! Wire-level behaviour of the embedded-SPA handler.
//!
//! These only ever ask for `index.html` (directly or through the SPA
//! fallback) or for a file no build contains: `index.html` is the one page
//! guaranteed to answer without a frontend build, which the handler then
//! replaces with its own placeholder. The policy decisions
//! the handler layers on top (which paths are immutable, which stored coding
//! to negotiate) are unit-tested in `src/web/assets.rs`.

use axum_test::TestServer;
use xuewen::db;

use crate::support;

async fn server() -> (tempfile::TempDir, TestServer) {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    let router = support::router(pool, dir.path().to_path_buf());
    let server = TestServer::new(router);
    (dir, server)
}

#[tokio::test]
async fn serves_the_shell_with_a_validator_and_a_revalidate_policy() {
    let (_dir, server) = server().await;
    let res = server.get("/").await;

    res.assert_status_ok();
    let etag = res.header("etag");
    let etag = etag.to_str().unwrap();
    // Strong: every stored coding of the asset carries a tag of its own.
    assert!(etag.starts_with('"'), "expected a strong ETag, got {etag}");
    assert_eq!(res.header("cache-control"), "public, no-cache");
    // Shared caches must key on the coding, or a proxy could hand a br body to
    // a client that never asked for one.
    assert_eq!(res.header("vary"), "accept-encoding");
}

#[tokio::test]
async fn a_known_etag_gets_a_bodiless_304() {
    let (_dir, server) = server().await;
    let etag = {
        let first = server.get("/").await;
        first.header("etag").to_str().unwrap().to_string()
    };

    let res = server
        .get("/")
        .add_header("if-none-match", etag.as_str())
        .await;

    res.assert_status(axum::http::StatusCode::NOT_MODIFIED);
    assert!(res.as_bytes().is_empty(), "a 304 must not carry a body");
    // A 304 has to repeat the caching headers, or the client cannot refresh
    // what it knows about the entry it just revalidated.
    assert_eq!(res.header("etag").to_str().unwrap(), etag);
    assert_eq!(res.header("cache-control"), "public, no-cache");
    assert_eq!(res.header("vary"), "accept-encoding");
}

#[tokio::test]
async fn a_stale_etag_gets_the_body_back() {
    let (_dir, server) = server().await;
    let res = server
        .get("/")
        .add_header("if-none-match", "W/\"0000000000000000\"")
        .await;

    res.assert_status_ok();
    assert!(!res.as_bytes().is_empty());
}

#[tokio::test]
async fn if_none_match_star_revalidates() {
    let (_dir, server) = server().await;
    let res = server.get("/").add_header("if-none-match", "*").await;
    res.assert_status(axum::http::StatusCode::NOT_MODIFIED);
}

#[tokio::test]
async fn if_none_match_accepts_a_list() {
    let (_dir, server) = server().await;
    let etag = {
        let first = server.get("/").await;
        first.header("etag").to_str().unwrap().to_string()
    };

    let res = server
        .get("/")
        .add_header("if-none-match", format!("W/\"other\", {etag}").as_str())
        .await;
    res.assert_status(axum::http::StatusCode::NOT_MODIFIED);
}

#[tokio::test]
async fn a_client_side_route_falls_back_to_the_shell() {
    let (_dir, server) = server().await;
    let root = server.get("/").await;
    let deep = server.get("/some/spa/route").await;

    deep.assert_status_ok();
    // Served *as* index.html — same validator, and index.html's revalidate
    // policy rather than whatever the deep link's own path would have implied.
    assert_eq!(
        deep.header("etag").to_str().unwrap(),
        root.header("etag").to_str().unwrap()
    );
    assert_eq!(deep.header("cache-control"), "public, no-cache");
}

#[tokio::test]
async fn a_declared_coding_always_matches_the_bytes_sent() {
    // The regression this pins: sending identity bytes while announcing
    // `content-encoding: br` makes the client decode garbage and end up with an
    // empty response. Holds with and without a frontend build — a built shell
    // has compressed siblings to send, the placeholder has none.
    let (_dir, server) = server().await;
    let identity = server.get("/").await.as_bytes().to_vec();
    assert!(!identity.is_empty());

    let res = server
        .get("/")
        .add_header("accept-encoding", "br, gzip")
        .await;
    res.assert_status_ok();
    let body = res.as_bytes().to_vec();
    match res.headers().get("content-encoding") {
        None => assert_eq!(body, identity, "no coding declared, so send it raw"),
        Some(coding) => assert_ne!(
            body, identity,
            "declared {coding:?} but sent the identity bytes"
        ),
    }
}

#[tokio::test]
async fn an_unacceptable_coding_is_served_identity() {
    let (_dir, server) = server().await;
    // `deflate` is not one we offer, and there is no `*` to fall back on.
    let res = server
        .get("/")
        .add_header("accept-encoding", "deflate")
        .await;

    res.assert_status_ok();
    assert!(
        res.headers().get("content-encoding").is_none(),
        "must not apply a coding the client did not offer"
    );
}

#[tokio::test]
async fn a_missing_file_is_a_404_not_the_shell() {
    let (_dir, server) = server().await;
    server
        .get("/assets/gone-1a2b3c.js")
        .expect_failure()
        .await
        .assert_status_not_found();
}
