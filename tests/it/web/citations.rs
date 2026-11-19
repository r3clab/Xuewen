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
    let (pool, root, _dir) = support::pool_and_root_with_paper("p1").await;
    // Plain router: no LLM anywhere - heuristics answer.
    let server = TestServer::new(support::router(pool, root));
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

    let (pool, root, _dir) = support::pool_and_root_with_paper("p1").await;
    let svc = xuewen::citations::CitationsService::for_tests(pool.clone(), &upstream.uri(), "m");
    let server = {
        let app = xuewen::app::App::builder(pool, root).citations(svc).build();
        TestServer::new(xuewen::web::router(app))
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
    let (pool, root, _dir) = support::pool_and_root_with_paper("p1").await;
    let server = TestServer::new(support::router(pool, root));
    let resp = server
        .post("/api/papers/nope/citations")
        .json(&json!({"references": ["x"]}))
        .await;
    resp.assert_status_not_found();
}

#[tokio::test]
async fn empty_references_is_bad_request() {
    let upstream = MockServer::start().await;
    let (pool, root, _dir) = support::pool_and_root_with_paper("p1").await;
    let svc = xuewen::citations::CitationsService::for_tests(pool.clone(), &upstream.uri(), "m");
    let server = {
        let app = xuewen::app::App::builder(pool, root).citations(svc).build();
        TestServer::new(xuewen::web::router(app))
    };

    let resp = server
        .post("/api/papers/p1/citations")
        .json(&json!({"references": []}))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn matches_references_against_the_library() {
    let (pool, root, _dir) = support::pool_and_root_with_paper("p1").await;
    xuewen::library::papers::insert(
        &pool,
        &xuewen::testutil::paper(
            "adam",
            "Adam: A Method for Stochastic Optimization",
            xuewen::library::model::PaperStatus::Resolved,
        ),
    )
    .await
    .unwrap();
    xuewen::library::tags::add_paper(
        &pool,
        "adam",
        &xuewen::library::tags::TagName::parse("optim").unwrap(),
    )
    .await
    .unwrap();
    let server = TestServer::new(support::router(pool, root));
    let resp = server
        .post("/api/citations/match")
        .json(&json!({"references": [
            {"text": "[1] Some Unrelated Paper About Frogs. Nature 2001."},
            {"text": "[2] D. Kingma, J. Ba. Adam: a method for stochastic optimization. ICLR 2015."},
            {"text": "[3] Kingma and Ba, ICLR 2015", "title": "Adam: A Method for Stochastic Optimization"},
        ]}))
        .await;
    resp.assert_status_ok();
    let v: serde_json::Value = resp.json();
    let matches = v["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 3);
    assert!(matches[0].is_null());
    assert_eq!(matches[1]["id"], "adam");
    assert_eq!(matches[1]["tags"][0]["name"], "optim");
    assert_eq!(matches[2]["id"], "adam");
}

#[tokio::test]
async fn matching_no_references_is_bad_request() {
    let (pool, root, _dir) = support::pool_and_root_with_paper("p1").await;
    let server = TestServer::new(support::router(pool, root));
    server
        .post("/api/citations/match")
        .json(&json!({"references": []}))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
}
