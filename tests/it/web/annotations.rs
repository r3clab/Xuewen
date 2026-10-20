use crate::support;

use axum_test::TestServer;
use serde_json::json;

/// Annotations are always on (no `[ai.*]` gate), so the plain router serves
/// them — there is no `build_router_with_annotations` to reach for.
async fn server(paper: &str) -> TestServer {
    let (pool, root) = support::pool_and_root_with_paper(paper).await;
    TestServer::new(support::router(pool, root)).unwrap()
}

fn highlight(page_index: i64, note: Option<&str>) -> serde_json::Value {
    let mut body = json!({
        "page_index": page_index,
        "kind": "highlight",
        "color": "amber",
        "quoted_text": "attention is all you need",
        "payload": { "annotation": { "type": 9, "segmentRects": [] } },
    });
    if let Some(n) = note {
        body["note"] = json!(n);
    }
    body
}

#[tokio::test]
async fn put_then_list_roundtrips_a_mark() {
    let server = server("p1").await;
    let resp = server
        .put("/api/papers/p1/annotations/a1")
        .json(&highlight(3, Some("the key claim")))
        .await;
    resp.assert_status_ok();
    let v: serde_json::Value = resp.json();
    assert_eq!(v["id"], "a1");
    assert_eq!(v["paper_id"], "p1");
    assert_eq!(v["page_index"], 3);
    assert_eq!(v["kind"], "highlight");
    assert_eq!(v["color"], "amber");
    assert_eq!(v["note"], "the key claim");
    // The opaque payload must come back byte-for-byte, not reshaped.
    assert_eq!(v["payload"]["annotation"]["type"], 9);

    let list: serde_json::Value = server.get("/api/papers/p1/annotations").await.json();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["id"], "a1");
}

#[tokio::test]
async fn list_is_empty_for_a_paper_with_no_marks() {
    let server = server("p1").await;
    let resp = server.get("/api/papers/p1/annotations").await;
    resp.assert_status_ok();
    assert_eq!(resp.json::<serde_json::Value>(), json!([]));
}

#[tokio::test]
async fn list_comes_back_in_reading_order() {
    let server = server("p1").await;
    for (id, page) in [("c", 5), ("a", 0), ("b", 2)] {
        server
            .put(&format!("/api/papers/p1/annotations/{id}"))
            .json(&highlight(page, None))
            .await
            .assert_status_ok();
    }
    let list: serde_json::Value = server.get("/api/papers/p1/annotations").await.json();
    let ids: Vec<_> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["a", "b", "c"]);
}

#[tokio::test]
async fn re_putting_the_same_id_replaces_instead_of_duplicating() {
    let server = server("p1").await;
    server
        .put("/api/papers/p1/annotations/a1")
        .json(&highlight(1, None))
        .await
        .assert_status_ok();
    let again = server
        .put("/api/papers/p1/annotations/a1")
        .json(&highlight(4, Some("second thoughts")))
        .await;
    again.assert_status_ok();

    let list: serde_json::Value = server.get("/api/papers/p1/annotations").await.json();
    assert_eq!(
        list.as_array().unwrap().len(),
        1,
        "a retried save must not duplicate"
    );
    assert_eq!(list[0]["page_index"], 4);
    assert_eq!(list[0]["note"], "second thoughts");
}

#[tokio::test]
async fn putting_a_blank_note_stores_null() {
    // Blank and never-set must be the same NULL, so a cleared note neither
    // lists as empty text nor reaches the search index.
    let server = server("p1").await;
    let resp = server
        .put("/api/papers/p1/annotations/a1")
        .json(&highlight(0, Some("   ")))
        .await;
    resp.assert_status_ok();
    assert_eq!(resp.json::<serde_json::Value>()["note"], json!(null));
}

#[tokio::test]
async fn delete_removes_one_mark_then_404s() {
    let server = server("p1").await;
    server
        .put("/api/papers/p1/annotations/a1")
        .json(&highlight(0, None))
        .await
        .assert_status_ok();
    server
        .delete("/api/papers/p1/annotations/a1")
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    server
        .delete("/api/papers/p1/annotations/a1")
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn clearing_a_paper_reports_how_many_went() {
    let server = server("p1").await;
    for id in ["a1", "a2", "a3"] {
        server
            .put(&format!("/api/papers/p1/annotations/{id}"))
            .json(&highlight(0, None))
            .await
            .assert_status_ok();
    }
    let resp = server.delete("/api/papers/p1/annotations").await;
    resp.assert_status_ok();
    assert_eq!(resp.json::<serde_json::Value>()["deleted"], 3);
    assert_eq!(
        server
            .get("/api/papers/p1/annotations")
            .await
            .json::<serde_json::Value>(),
        json!([])
    );
}

#[tokio::test]
async fn unknown_paper_is_404_on_every_verb() {
    let server = server("p1").await;
    server
        .get("/api/papers/nope/annotations")
        .await
        .assert_status_not_found();
    server
        .put("/api/papers/nope/annotations/a1")
        .json(&highlight(0, None))
        .await
        .assert_status_not_found();
    server
        .delete("/api/papers/nope/annotations")
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn a_negative_page_is_rejected() {
    let server = server("p1").await;
    server
        .put("/api/papers/p1/annotations/a1")
        .json(&highlight(-1, None))
        .await
        .assert_status_bad_request();
}

#[tokio::test]
async fn a_non_object_payload_is_rejected() {
    let server = server("p1").await;
    let mut body = highlight(0, None);
    body["payload"] = json!([1, 2, 3]);
    server
        .put("/api/papers/p1/annotations/a1")
        .json(&body)
        .await
        .assert_status_bad_request();
}

#[tokio::test]
async fn an_unrenderable_kind_is_refused_at_the_wire() {
    // The closed AnnotationKind enum is what keeps a subtype the reader has
    // no renderer for out of storage; serde rejects it before any handler
    // code runs. That lands as 422 (the body parses as JSON but names a
    // variant that doesn't exist), not the 400 our own validators return.
    let server = server("p1").await;
    let mut body = highlight(0, None);
    body["kind"] = json!("ink");
    server
        .put("/api/papers/p1/annotations/a1")
        .json(&body)
        .await
        .assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn annotations_are_scoped_to_their_paper() {
    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    // A second paper sharing the same database must not see p1's marks.
    xuewen::library::papers::insert(
        &pool,
        &xuewen::library::model::Paper {
            id: "p2".into(),
            content_hash: "p2".into(),
            rel_path: "other.pdf".into(),
            cite_key: Some("p2".into()),
            added_at: "2026-07-07T00:00:00Z".into(),
            deleted_at: None,
            starred: false,
            name: None,
            meta: xuewen::library::model::PaperMeta {
                title: Some("Paper p2".into()),
                abstract_text: None,
                authors: xuewen::library::model::Authors(vec![]),
                venue: None,
                year: None,
                doi: None,
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: xuewen::library::model::PaperStatus::Resolved,
            },
        },
    )
    .await
    .unwrap();
    let server = TestServer::new(support::router(pool, root)).unwrap();

    server
        .put("/api/papers/p1/annotations/shared-id")
        .json(&highlight(0, Some("belongs to p1")))
        .await
        .assert_status_ok();
    // The same annotation id on another paper is a different row, not a clash.
    server
        .put("/api/papers/p2/annotations/shared-id")
        .json(&highlight(0, Some("belongs to p2")))
        .await
        .assert_status_ok();

    let p1: serde_json::Value = server.get("/api/papers/p1/annotations").await.json();
    let p2: serde_json::Value = server.get("/api/papers/p2/annotations").await.json();
    assert_eq!(p1.as_array().unwrap().len(), 1);
    assert_eq!(p1[0]["note"], "belongs to p1");
    assert_eq!(p2[0]["note"], "belongs to p2");

    // Deleting one paper's mark leaves the other's alone.
    server
        .delete("/api/papers/p1/annotations/shared-id")
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    let p2_after: serde_json::Value = server.get("/api/papers/p2/annotations").await.json();
    assert_eq!(p2_after.as_array().unwrap().len(), 1);
}
