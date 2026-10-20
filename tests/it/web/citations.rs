use crate::support;

use axum_test::TestServer;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn chat_reply(text: &str) -> serde_json::Value {
    json!({"choices": [{"message": {"role": "assistant", "content": text}}]})
}

#[tokio::test]
async fn parses_references_heuristically_without_llm() {
    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    // Plain router: no LLM anywhere - heuristics answer.
    let server = TestServer::new(support::router(pool, root)).unwrap();
    let body = json!({"references": [
        "[1] D. Kingma and J. Ba, \"Adam: A method for stochastic optimization,\" in Proc. of ICLR, 2015."
    ]});
    let resp = server.post("/api/papers/p1/citations").json(&body).await;
    resp.assert_status_ok();
    let v: serde_json::Value = resp.json();
    assert_eq!(
        v["references"][0]["title"],
        "Adam: A method for stochastic optimization"
    );
    assert_eq!(v["references"][0]["year"], 2015);
}

#[tokio::test]
async fn parses_leftovers_via_llm_and_caches() {
    let upstream = MockServer::start().await;
    let parsed = r#"[{"i":1,"authors":["A. Author"],"title":"Recovered","venue":null,"year":2020,"doi":null,"arxiv_id":null,"url":null}]"#;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply(parsed)))
        .expect(1) // second POST below must hit the cache
        .mount(&upstream)
        .await;

    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    let svc = xuewen::citations::CitationsService::for_tests(pool.clone(), &upstream.uri(), "m");
    let server = {
        let app = xuewen::app::App::builder(pool, root).citations(svc).build();
        TestServer::new(xuewen::web::router(app)).unwrap()
    };

    // Heuristically unparseable (no anchors, no year) -> goes to the LLM.
    let body = json!({"references": ["%% garbled fragment %%"]});
    let resp = server.post("/api/papers/p1/citations").json(&body).await;
    resp.assert_status_ok();
    let v: serde_json::Value = resp.json();
    assert_eq!(v["references"][0]["title"], "Recovered");

    let resp2 = server.post("/api/papers/p1/citations").json(&body).await;
    resp2.assert_status_ok();
}

#[tokio::test]
async fn unknown_paper_is_404() {
    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    let server = TestServer::new(support::router(pool, root)).unwrap();
    let resp = server
        .post("/api/papers/nope/citations")
        .json(&json!({"references": ["x"]}))
        .await;
    resp.assert_status_not_found();
}

#[tokio::test]
async fn empty_references_is_bad_request() {
    let upstream = MockServer::start().await;
    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    let svc = xuewen::citations::CitationsService::for_tests(pool.clone(), &upstream.uri(), "m");
    let server = {
        let app = xuewen::app::App::builder(pool, root).citations(svc).build();
        TestServer::new(xuewen::web::router(app)).unwrap()
    };

    let resp = server
        .post("/api/papers/p1/citations")
        .json(&json!({"references": []}))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}
