use crate::support;

use axum_test::multipart::{MultipartForm, Part};
use axum_test::TestServer;
use wiremock::matchers::{method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xuewen::app::App;
use xuewen::db;
use xuewen::library::model::PaperStatus;
use xuewen::library::projects::{self, ProjectName};
use xuewen::library::tags::{self, TagName};
use xuewen::library::{papers, trash};
use xuewen::metadata::resolve::Resolver;
use xuewen::testutil::paper;

const DBLP_FIXTURE: &str = support::fixture!("dblp_kgat.json");
const CROSSREF_FIXTURE: &str = support::fixture!("crossref_kgat.json");
const ARXIV_FIXTURE: &str = support::fixture!("arxiv_attention.xml");

#[tokio::test]
async fn lists_and_details_papers() {
    let (dir, pool) = support::temp_pool().await;
    papers::insert(
        &pool,
        &paper("aaaa1111", "Deep Residual Learning", PaperStatus::Resolved),
    )
    .await
    .unwrap();
    papers::insert(
        &pool,
        &paper(
            "bbbb2222",
            "Attention Is All You Need",
            PaperStatus::NeedsReview,
        ),
    )
    .await
    .unwrap();
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();

    // List: JSON array of summaries, authors as an array, no abstract field.
    let resp = server.get("/api/papers").await;
    resp.assert_status_ok();
    let list: Vec<serde_json::Value> = resp.json();
    assert_eq!(list.len(), 2);
    assert!(list[0]["authors"].is_array());
    assert!(list[0].get("abstract").is_none());

    // Search filter.
    let resp = server.get("/api/papers?q=attention").await;
    let hits: Vec<serde_json::Value> = resp.json();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["id"], "bbbb2222");

    // Detail includes abstract.
    let resp = server.get("/api/papers/aaaa1111").await;
    resp.assert_status_ok();
    let detail: serde_json::Value = resp.json();
    assert_eq!(detail["abstract"], "An abstract.");
    assert_eq!(detail["cite_key"], "aaaa1111");

    // Unknown id → 404.
    server
        .get("/api/papers/nope")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // Stats.
    let resp = server.get("/api/stats").await;
    let s: serde_json::Value = resp.json();
    assert_eq!(s["total"], 2);
    assert_eq!(s["resolved"], 1);
    assert_eq!(s["needs_review"], 1);
}

#[tokio::test]
async fn paper_detail_includes_summary_when_present() {
    let (dir, pool) = support::temp_pool().await;
    papers::insert(
        &pool,
        &paper("aaaa1111", "Deep Residual Learning", PaperStatus::Resolved),
    )
    .await
    .unwrap();
    papers::insert(
        &pool,
        &paper(
            "bbbb2222",
            "Attention Is All You Need",
            PaperStatus::Resolved,
        ),
    )
    .await
    .unwrap();

    let summary = xuewen::summary::Summary {
        tldr: "A one-line summary.".into(),
        problem: "The identified gap.".into(),
        approach: "The proposed method.".into(),
        results: "+4.2 over baseline.".into(),
        limitations: "Small dataset.".into(),
    };
    xuewen::summary::store::upsert(&pool, "aaaa1111", &summary, "test-model")
        .await
        .unwrap();

    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();

    // Paper with a stored summary: the `summary` key is present with the
    // expected fields.
    let resp = server.get("/api/papers/aaaa1111").await;
    resp.assert_status_ok();
    let detail: serde_json::Value = resp.json();
    assert_eq!(detail["summary"]["tldr"], "A one-line summary.");
    assert_eq!(detail["summary"]["results"], "+4.2 over baseline.");

    // Paper with no summary: the `summary` key is absent entirely (serde
    // `skip_serializing_if` omits it, rather than serializing `null`).
    let resp = server.get("/api/papers/bbbb2222").await;
    resp.assert_status_ok();
    let detail: serde_json::Value = resp.json();
    assert!(detail.get("summary").is_none());
}

#[tokio::test]
async fn streams_pdf_with_range_and_guards_paths() {
    let (dir, pool) = support::temp_pool().await;
    let library = dir.path().join("library");
    std::fs::create_dir_all(&library).unwrap();

    // A real paper whose PDF exists inside the library.
    let mut ok = paper("cccc3333", "A Paper", PaperStatus::Resolved);
    ok.rel_path = "cccc3333.pdf".into();
    support::write_test_pdf(&library.join("cccc3333.pdf"), &["Hello PDF"]);
    papers::insert(&pool, &ok).await.unwrap();

    // A rogue record whose rel_path escapes the library.
    let mut escape = paper("dddd4444", "Escape", PaperStatus::Resolved);
    escape.rel_path = "../outside.pdf".into();
    std::fs::write(dir.path().join("outside.pdf"), b"secret").unwrap();
    papers::insert(&pool, &escape).await.unwrap();

    let server = TestServer::new(support::router(pool, library.clone())).unwrap();

    // Full GET → 200, application/pdf.
    let resp = server.get("/papers/cccc3333/pdf").await;
    resp.assert_status_ok();
    assert_eq!(
        resp.header("content-type").to_str().unwrap(),
        "application/pdf"
    );
    let full_len = resp.as_bytes().len();
    assert!(full_len > 0);

    // Range request → 206 Partial Content, 100 bytes.
    let resp = server
        .get("/papers/cccc3333/pdf")
        .add_header(axum::http::header::RANGE, "bytes=0-99")
        .await;
    resp.assert_status(axum::http::StatusCode::PARTIAL_CONTENT);
    assert_eq!(resp.as_bytes().len(), 100);

    // Missing id → 404.
    server
        .get("/papers/zzzz9999/pdf")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // Path-escaping record → 404 (guard rejects it, does NOT serve outside file).
    server
        .get("/papers/dddd4444/pdf")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn restores_a_deleted_paper() {
    let (dir, pool) = support::temp_pool().await;
    papers::insert(&pool, &paper("aaaa1111", "First", PaperStatus::Resolved))
        .await
        .unwrap();
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();

    server
        .delete("/api/papers/aaaa1111")
        .await
        .assert_status_ok();
    assert_eq!(
        server
            .get("/api/papers")
            .await
            .json::<Vec<serde_json::Value>>()
            .len(),
        0
    );

    // POST restore → back in the active list.
    server
        .post("/api/papers/aaaa1111/restore")
        .await
        .assert_status_ok();
    assert_eq!(
        server
            .get("/api/papers")
            .await
            .json::<Vec<serde_json::Value>>()
            .len(),
        1
    );

    // Restoring an already-active or unknown paper → 404.
    server
        .post("/api/papers/aaaa1111/restore")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
    server
        .post("/api/papers/zzzz9999/restore")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deletes_a_paper_softly() {
    let (dir, pool) = support::temp_pool().await;
    papers::insert(&pool, &paper("aaaa1111", "First", PaperStatus::Resolved))
        .await
        .unwrap();
    papers::insert(
        &pool,
        &paper("bbbb2222", "Second", PaperStatus::NeedsReview),
    )
    .await
    .unwrap();
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();

    // Before: both listed.
    assert_eq!(
        server
            .get("/api/papers")
            .await
            .json::<Vec<serde_json::Value>>()
            .len(),
        2
    );

    // DELETE one → 200, and it drops out of the active list + stats.
    server
        .delete("/api/papers/aaaa1111")
        .await
        .assert_status_ok();
    let list = server
        .get("/api/papers")
        .await
        .json::<Vec<serde_json::Value>>();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], "bbbb2222");
    assert_eq!(
        server.get("/api/stats").await.json::<serde_json::Value>()["total"],
        1
    );

    // DELETE an unknown id → 404.
    server
        .delete("/api/papers/nope")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

/// Pins the `Trash::Allow` choices in web/handlers: a trashed paper's detail,
/// export, and annotations stay reachable (an open reader tab on a
/// just-trashed paper keeps working; marks survive a restore). Chat/code
/// deny trashed papers — their own test files pin that.
#[tokio::test]
async fn trashed_paper_detail_export_and_annotations_stay_reachable() {
    let (dir, pool) = support::temp_pool().await;
    papers::insert(&pool, &paper("aaaa1111", "First", PaperStatus::Resolved))
        .await
        .unwrap();
    trash::soft_delete(&pool, "aaaa1111").await.unwrap();
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();

    server.get("/api/papers/aaaa1111").await.assert_status_ok();
    server
        .get("/api/papers/aaaa1111/export")
        .await
        .assert_status_ok();
    server
        .get("/api/papers/aaaa1111/annotations")
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn imports_a_pdf_dedups_and_rejects_non_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();

    // Offline resolver: upstreams refuse instantly, so resolution degrades to
    // needs_review with no network wait (same trick as the watcher tests).
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string());

    let ingest = support::ingest(
        pool.clone(),
        dir.path(),
        resolver,
        xuewen::ingest::fetch::Fetcher::new(None).unwrap(),
    );
    let server = TestServer::new(xuewen::web::router(
        App::builder(pool, library.clone()).ingest(ingest).build(),
    ))
    .unwrap();

    // A real one-page PDF whose header has no DOI/arXiv id.
    let pdf_path = dir.path().join("paper.pdf");
    support::write_test_pdf(&pdf_path, &["A Paper With No Identifier"]);
    let pdf_bytes = std::fs::read(&pdf_path).unwrap();

    // Import -> 200 ingested, needs_review.
    let form = MultipartForm::new().add_part(
        "file",
        Part::bytes(pdf_bytes.clone()).file_name("paper.pdf"),
    );
    let resp = server.post("/api/papers").multipart(form).await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["outcome"], "ingested");
    assert_eq!(body["status"], "needs_review");

    // It now shows up in the list and the stats.
    assert_eq!(
        server
            .get("/api/papers")
            .await
            .json::<Vec<serde_json::Value>>()
            .len(),
        1
    );
    assert_eq!(
        server.get("/api/stats").await.json::<serde_json::Value>()["total"],
        1
    );

    // Re-import identical bytes -> 200 duplicate.
    let form2 =
        MultipartForm::new().add_part("file", Part::bytes(pdf_bytes).file_name("paper.pdf"));
    let dup: serde_json::Value = server.post("/api/papers").multipart(form2).await.json();
    assert_eq!(dup["outcome"], "duplicate");

    // Non-PDF bytes -> 400.
    let form3 = MultipartForm::new().add_part(
        "file",
        Part::bytes(b"not a pdf".to_vec()).file_name("x.pdf"),
    );
    server
        .post("/api/papers")
        .multipart(form3)
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn import_sanitizes_traversal_filenames() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string());
    let ingest = support::ingest(
        pool.clone(),
        dir.path(),
        resolver,
        xuewen::ingest::fetch::Fetcher::new(None).unwrap(),
    );
    let server = TestServer::new(xuewen::web::router(
        App::builder(pool, library.clone()).ingest(ingest).build(),
    ))
    .unwrap();

    let pdf_path = dir.path().join("p.pdf");
    support::write_test_pdf(&pdf_path, &["Traversal Test Paper"]);
    let bytes = std::fs::read(&pdf_path).unwrap();

    // A filename containing path separators must be reduced to its basename:
    // the upload still succeeds and nothing is written outside the staging dir.
    let form =
        MultipartForm::new().add_part("file", Part::bytes(bytes).file_name("nested/evil.pdf"));
    let resp = server.post("/api/papers").multipart(form).await;
    resp.assert_status_ok();
    assert_eq!(resp.json::<serde_json::Value>()["outcome"], "ingested");
    // No traversal artifacts landed outside the _uploads staging dir.
    assert!(!inbox.join("evil.pdf").exists());
    assert!(!inbox.join("nested").exists());
}

#[tokio::test]
async fn import_returns_503_when_not_configured() {
    let (dir, pool) = support::temp_pool().await;
    // The read-only router (no ingest bundle) must refuse uploads.
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();
    let form = MultipartForm::new().add_part(
        "file",
        Part::bytes(b"%PDF-1.4\n".to_vec()).file_name("x.pdf"),
    );
    server
        .post("/api/papers")
        .multipart(form)
        .await
        .assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn import_reports_in_trash_for_deleted_paper() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string());
    let ingest = support::ingest(
        pool.clone(),
        dir.path(),
        resolver,
        xuewen::ingest::fetch::Fetcher::new(None).unwrap(),
    );
    let server = TestServer::new(xuewen::web::router(
        App::builder(pool.clone(), library.clone())
            .ingest(ingest)
            .build(),
    ))
    .unwrap();

    let pdf_path = dir.path().join("paper.pdf");
    support::write_test_pdf(&pdf_path, &["A Paper With No Identifier"]);
    let pdf_bytes = std::fs::read(&pdf_path).unwrap();

    let form = MultipartForm::new().add_part(
        "file",
        Part::bytes(pdf_bytes.clone()).file_name("paper.pdf"),
    );
    let body: serde_json::Value = server.post("/api/papers").multipart(form).await.json();
    assert_eq!(body["outcome"], "ingested");
    let id = body["id"].as_str().unwrap().to_string();

    trash::soft_delete(&pool, &id).await.unwrap();

    // Re-upload the same bytes → in_trash with the trashed paper's id.
    let form2 =
        MultipartForm::new().add_part("file", Part::bytes(pdf_bytes).file_name("paper.pdf"));
    let body2: serde_json::Value = server.post("/api/papers").multipart(form2).await.json();
    assert_eq!(body2["outcome"], "in_trash");
    assert_eq!(body2["id"], serde_json::json!(id));
}

#[tokio::test]
async fn import_reports_same_work_for_known_doi() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();

    // Seed an active paper that already owns this DOI.
    let mut existing = paper(
        "01890000-0000-7000-8000-0000000000aa",
        "Seed",
        PaperStatus::Resolved,
    );
    existing.meta.doi = Some("10.1000/xyz123".into());
    papers::insert(&pool, &existing).await.unwrap();

    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string());
    let ingest = support::ingest(
        pool.clone(),
        dir.path(),
        resolver,
        xuewen::ingest::fetch::Fetcher::new(None).unwrap(),
    );
    let server = TestServer::new(xuewen::web::router(
        App::builder(pool, library.clone()).ingest(ingest).build(),
    ))
    .unwrap();

    // Upload a different file whose first page carries the same DOI. The
    // resolver is offline, but the extracted identifier still lands in the
    // decided fields, so the identifier dedup fires.
    let pdf_path = dir.path().join("other.pdf");
    support::write_test_pdf(
        &pdf_path,
        &["A Different Upload", "https://doi.org/10.1000/xyz123"],
    );
    let form = MultipartForm::new().add_part(
        "file",
        Part::bytes(std::fs::read(&pdf_path).unwrap()).file_name("other.pdf"),
    );
    let body: serde_json::Value = server.post("/api/papers").multipart(form).await.json();
    assert_eq!(body["outcome"], "same_work");
    assert_eq!(body["id"], serde_json::json!(existing.id));
}

#[tokio::test]
async fn identify_search_returns_candidates() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DBLP_FIXTURE))
        .mount(&server)
        .await;
    // Crossref search 404s -> degrade to DBLP-only candidates.
    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());
    let ingest = support::ingest(
        pool.clone(),
        dir.path(),
        resolver,
        xuewen::ingest::fetch::Fetcher::new(None).unwrap(),
    );
    let server_http = TestServer::new(xuewen::web::router(
        App::builder(pool, library.clone()).ingest(ingest).build(),
    ))
    .unwrap();

    let body: serde_json::Value = server_http
        .get("/api/identify/search")
        .add_query_param("q", "KGAT Knowledge Graph")
        .await
        .json();
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(
        arr[0]["title"],
        "KGAT: Knowledge Graph Attention Network for Recommendation"
    );
    assert_eq!(arr[0]["source"], "dblp");
    assert!(arr[0]["authors"].is_array());

    // Empty q -> 400.
    server_http
        .get("/api/identify/search")
        .add_query_param("q", "  ")
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn identify_search_needs_ingest_context() {
    let (dir, pool) = support::temp_pool().await;
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();
    server
        .get("/api/identify/search")
        .add_query_param("q", "anything")
        .await
        .assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn identify_applies_doi_candidate_and_guards() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();

    // Crossref mock for the direct-DOI path.
    let doi = "10.1145/3292500.3330701";
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&mock)
        .await;
    let resolver = Resolver::with_bases(None, mock.uri(), mock.uri())
        .unwrap()
        .with_dblp_base(mock.uri());

    // Seed a needs_review paper whose PDF exists in the library.
    let mut p = paper(
        "01890000-0000-7000-8000-0000000000a1",
        "Provisional",
        PaperStatus::NeedsReview,
    );
    std::fs::create_dir_all(library.join("_unsorted")).unwrap();
    p.rel_path = format!("_unsorted/{}.pdf", p.content_hash);
    std::fs::write(library.join(&p.rel_path), b"%PDF-1.4 fake").unwrap();
    papers::insert(&pool, &p).await.unwrap();

    let ingest = support::ingest(
        pool.clone(),
        dir.path(),
        resolver,
        xuewen::ingest::fetch::Fetcher::new(None).unwrap(),
    );
    let server = TestServer::new(xuewen::web::router(
        App::builder(pool.clone(), library.clone())
            .ingest(ingest)
            .build(),
    ))
    .unwrap();

    // Direct DOI identify: 200, resolved, re-filed to the cite-key path.
    let resp = server
        .post(&format!("/api/papers/{}/identify", p.id))
        .json(&serde_json::json!({ "doi": doi }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["status"], "resolved");
    assert_eq!(
        body["title"],
        "KGAT: Knowledge Graph Attention Network for Recommendation"
    );
    assert_eq!(body["cite_key"], "wang2019kgat");
    assert!(library.join("wang2019kgat.pdf").exists());

    // Unknown paper id -> 404.
    server
        .post("/api/papers/01890000-0000-7000-8000-00000000dead/identify")
        .json(&serde_json::json!({ "doi": doi }))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // A second paper trying to claim the SAME DOI -> 409 naming the winner.
    let mut q = paper(
        "01890000-0000-7000-8000-0000000000b2",
        "Other",
        PaperStatus::NeedsReview,
    );
    q.content_hash = "otherhash".into();
    q.rel_path = "_unsorted/otherhash.pdf".into();
    std::fs::write(library.join(&q.rel_path), b"%PDF-1.4 other").unwrap();
    papers::insert(&pool, &q).await.unwrap();
    let resp = server
        .post(&format!("/api/papers/{}/identify", q.id))
        .json(&serde_json::json!({ "doi": doi }))
        .await;
    resp.assert_status(axum::http::StatusCode::CONFLICT);
    let body: serde_json::Value = resp.json();
    assert_eq!(body["id"], serde_json::json!(p.id));

    // Trashed paper -> 409.
    trash::soft_delete(&pool, &q.id).await.unwrap();
    server
        .post(&format!("/api/papers/{}/identify", q.id))
        .json(&serde_json::json!({ "candidate": {
            "title": "T", "abstract": null, "authors": [], "venue": null,
            "year": null, "doi": null, "arxiv_id": null, "dblp_key": null,
            "url": null, "source": "dblp"
        }}))
        .await
        .assert_status(axum::http::StatusCode::CONFLICT);

    // Bad body (two selectors) -> 400.
    trash::restore(&pool, &q.id).await.unwrap();
    server
        .post(&format!("/api/papers/{}/identify", q.id))
        .json(&serde_json::json!({ "doi": "10.1/x", "arxiv_id": "2001.00001" }))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);

    // Unresolvable DOI (mock has no route for it -> 404 upstream) -> 404,
    // with an actionable hint pointing at the title-search path.
    let resp = server
        .post(&format!("/api/papers/{}/identify", q.id))
        .json(&serde_json::json!({ "doi": "10.9999/nope" }))
        .await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
    let body: serde_json::Value = resp.json();
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("try a title search"),
        "not-found error should suggest the title path, got: {body}"
    );
}

#[tokio::test]
async fn identify_applies_candidate_without_network() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string());

    let mut p = paper(
        "01890000-0000-7000-8000-00000000c001",
        "Prov",
        PaperStatus::NeedsReview,
    );
    std::fs::create_dir_all(library.join("_unsorted")).unwrap();
    p.rel_path = format!("_unsorted/{}.pdf", p.content_hash);
    std::fs::write(library.join(&p.rel_path), b"%PDF-1.4 fake").unwrap();
    papers::insert(&pool, &p).await.unwrap();

    let ingest = support::ingest(
        pool.clone(),
        dir.path(),
        resolver,
        xuewen::ingest::fetch::Fetcher::new(None).unwrap(),
    );
    let server = TestServer::new(xuewen::web::router(
        App::builder(pool, library.clone()).ingest(ingest).build(),
    ))
    .unwrap();

    let resp = server
        .post(&format!("/api/papers/{}/identify", p.id))
        .json(&serde_json::json!({ "candidate": {
            "title": "AntiFuzz: Impeding Fuzzing Audits of Binary Executables",
            "abstract": null,
            "authors": ["Emre Güler", "Cornelius Aschermann", "Ali Abbasi", "Thorsten Holz"],
            "venue": "USENIX Security Symposium",
            "year": 2019,
            "doi": null, "arxiv_id": null,
            "dblp_key": "conf/uss/GulerAAH19",
            "url": "https://www.usenix.org/conference/usenixsecurity19/presentation/guler",
            "source": "dblp"
        }}))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["status"], "resolved");
    assert_eq!(body["cite_key"], "guler2019antifuzz");
    assert_eq!(body["source"], "dblp");
    assert!(library.join("guler2019antifuzz.pdf").exists());
}

#[tokio::test]
async fn identify_needs_ingest_context() {
    let (dir, pool) = support::temp_pool().await;
    // The read-only router (no ingest bundle) must refuse identify writes.
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();
    server
        .post("/api/papers/whatever/identify")
        .json(&serde_json::json!({ "doi": "10.1/x" }))
        .await
        .assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn identify_applies_arxiv_id_via_api() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();

    // arXiv mock for the direct-id path.
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/api/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ARXIV_FIXTURE))
        .mount(&mock)
        .await;
    let resolver = Resolver::with_bases(None, mock.uri(), mock.uri())
        .unwrap()
        .with_dblp_base(mock.uri());

    let mut p = paper(
        "01890000-0000-7000-8000-00000000d001",
        "Prov",
        PaperStatus::NeedsReview,
    );
    std::fs::create_dir_all(library.join("_unsorted")).unwrap();
    p.rel_path = format!("_unsorted/{}.pdf", p.content_hash);
    std::fs::write(library.join(&p.rel_path), b"%PDF-1.4 fake").unwrap();
    papers::insert(&pool, &p).await.unwrap();

    let ingest = support::ingest(
        pool.clone(),
        dir.path(),
        resolver,
        xuewen::ingest::fetch::Fetcher::new(None).unwrap(),
    );
    let server = TestServer::new(xuewen::web::router(
        App::builder(pool, library.clone()).ingest(ingest).build(),
    ))
    .unwrap();

    let resp = server
        .post(&format!("/api/papers/{}/identify", p.id))
        .json(&serde_json::json!({ "arxiv_id": "1706.03762" }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["status"], "resolved");
    assert_eq!(body["title"], "Attention Is All You Need");
    assert_eq!(body["cite_key"], "vaswani2017attention");
    assert!(library.join("vaswani2017attention.pdf").exists());
}

#[tokio::test]
async fn settings_report_and_set_proxy_cookie() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".into(),
        "http://127.0.0.1:1".into(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".into());
    let ingest = support::ingest(
        pool.clone(),
        dir.path(),
        resolver,
        // The proxy prefix now rides in the fetcher, mirroring production.
        xuewen::ingest::fetch::Fetcher::new(Some("https://proxy.uchicago.edu/login?url=".into()))
            .unwrap(),
    );
    let server = TestServer::new(xuewen::web::router(
        App::builder(pool.clone(), library.clone())
            .ingest(ingest)
            .build(),
    ))
    .unwrap();

    // Initially unset; the configured proxy host is reported so the UI can
    // name it instead of hardcoding one deployment's university.
    let s: serde_json::Value = server.get("/api/settings").await.json();
    assert_eq!(s["proxy_cookie_set"], false);
    assert_eq!(s["proxy"]["host"], "proxy.uchicago.edu");

    // Set it.
    server
        .put("/api/settings/proxy-cookie")
        .json(&serde_json::json!({ "cookie": "ezproxy=abc" }))
        .await
        .assert_status_ok();
    let s: serde_json::Value = server.get("/api/settings").await.json();
    assert_eq!(s["proxy_cookie_set"], true);
    assert!(s["proxy_cookie_updated_at"].is_string());
    // The value is never echoed.
    assert!(s.get("cookie").is_none());

    // Clear it.
    server
        .delete("/api/settings/proxy-cookie")
        .await
        .assert_status_ok();
    let s: serde_json::Value = server.get("/api/settings").await.json();
    assert_eq!(s["proxy_cookie_set"], false);
}

#[tokio::test]
async fn settings_reports_fold_abstract() {
    // The read-only test router builds its App with `UiConfig::default()`
    // (fold_abstract: true), so this exercises the default straight through.
    let (dir, pool) = support::temp_pool().await;
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();

    let res = server.get("/api/settings").await;
    res.assert_status_ok();
    let s: serde_json::Value = res.json();
    assert_eq!(s["fold_abstract"], serde_json::json!(true));
    // No ingest/[proxy] here -> `proxy` is null and the UI hides
    // institutional access.
    assert!(s["proxy"].is_null());
}

#[tokio::test]
async fn import_url_rejects_unsupported_input() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".into(),
        "http://127.0.0.1:1".into(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".into());
    let ingest = support::ingest(
        pool.clone(),
        dir.path(),
        resolver,
        xuewen::ingest::fetch::Fetcher::new(None).unwrap(),
    );
    let server = TestServer::new(xuewen::web::router(
        App::builder(pool, library).ingest(ingest).build(),
    ))
    .unwrap();

    server
        .post("/api/import")
        .json(&serde_json::json!({ "input": "just a title, not an id" }))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn projects_crud_membership_and_filter() {
    let (dir, pool) = support::temp_pool().await;
    papers::insert(
        &pool,
        &paper("aaaa1111", "Deep Residual Learning", PaperStatus::Resolved),
    )
    .await
    .unwrap();
    papers::insert(
        &pool,
        &paper(
            "bbbb2222",
            "Attention Is All You Need",
            PaperStatus::Resolved,
        ),
    )
    .await
    .unwrap();
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();

    // Create.
    let resp = server
        .post("/api/projects")
        .json(&serde_json::json!({"name": "Survey"}))
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let proj: serde_json::Value = resp.json();
    let pid = proj["id"].as_str().unwrap().to_string();
    assert_eq!(proj["name"], "Survey");

    // Duplicate name -> 409.
    server
        .post("/api/projects")
        .json(&serde_json::json!({"name": "survey"}))
        .await
        .assert_status(axum::http::StatusCode::CONFLICT);

    // Add membership (idempotent) -> 204.
    server
        .put(&format!("/api/papers/aaaa1111/projects/{pid}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    server
        .put(&format!("/api/papers/aaaa1111/projects/{pid}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    // Membership to a missing paper -> 404.
    server
        .put(&format!("/api/papers/nope/projects/{pid}"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // List shows count 1.
    let list: Vec<serde_json::Value> = server.get("/api/projects").await.json();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["paper_count"], 1);

    // Detail carries projects (objects, not bare ids).
    let detail: serde_json::Value = server.get("/api/papers/aaaa1111").await.json();
    assert_eq!(
        detail["projects"],
        serde_json::json!([{"id": pid, "name": "Survey"}])
    );

    // Filter list by project.
    let filtered: Vec<serde_json::Value> = server
        .get(&format!("/api/papers?project={pid}"))
        .await
        .json();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0]["id"], "aaaa1111");

    // Remove membership -> 204, then 404 when absent.
    server
        .delete(&format!("/api/papers/aaaa1111/projects/{pid}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    server
        .delete(&format!("/api/papers/aaaa1111/projects/{pid}"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // Delete project -> 204.
    server
        .delete(&format!("/api/projects/{pid}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn patch_project_merge_rules() {
    let (dir, pool) = support::temp_pool().await;
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();

    // Seed two projects: one to edit, one to collide names with.
    let a: serde_json::Value = server
        .post("/api/projects")
        .json(&serde_json::json!({"name": "Survey"}))
        .await
        .json();
    let a_id = a["id"].as_str().unwrap().to_string();
    server
        .post("/api/projects")
        .json(&serde_json::json!({"name": "Other"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    // (a) Rename → 200, name changed.
    let resp = server
        .patch(&format!("/api/projects/{a_id}"))
        .json(&serde_json::json!({"name": "Survey v2"}))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["name"], "Survey v2");

    // (d) Rename onto another project's name → 409.
    server
        .patch(&format!("/api/projects/{a_id}"))
        .json(&serde_json::json!({"name": "other"}))
        .await
        .assert_status(axum::http::StatusCode::CONFLICT);

    // (e) PATCH an unknown id → 404.
    server
        .patch("/api/projects/does-not-exist")
        .json(&serde_json::json!({"name": "whatever"}))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tags_crud_membership_and_gc() {
    let (dir, pool) = support::temp_pool().await;
    papers::insert(
        &pool,
        &paper("aaaa1111", "Deep Residual Learning", PaperStatus::Resolved),
    )
    .await
    .unwrap();
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();

    // Add (create-if-missing) -> 200 with {id,name}.
    let resp = server
        .put("/api/papers/aaaa1111/tags")
        .json(&serde_json::json!({"name": "nlp"}))
        .await;
    resp.assert_status_ok();
    let tag: serde_json::Value = resp.json();
    let tag_id = tag["id"].as_str().unwrap().to_string();
    assert_eq!(tag["name"], "nlp");

    // Empty/blank name -> 400.
    server
        .put("/api/papers/aaaa1111/tags")
        .json(&serde_json::json!({"name": "   "}))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);

    // Tagging a missing paper -> 404 (clean, not an FK-violation 500).
    server
        .put("/api/papers/nope/tags")
        .json(&serde_json::json!({"name": "nlp"}))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // List shows it with count 1.
    let list: Vec<serde_json::Value> = server.get("/api/tags").await.json();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], tag_id);
    assert_eq!(list[0]["paper_count"], 1);

    // Remove membership -> 204, and the now-orphaned tag is GC'd.
    server
        .delete(&format!("/api/papers/aaaa1111/tags/{tag_id}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    let list: Vec<serde_json::Value> = server.get("/api/tags").await.json();
    assert!(list.is_empty());
}

#[tokio::test]
async fn tags_rename_and_delete() {
    let (dir, pool) = support::temp_pool().await;
    papers::insert(
        &pool,
        &paper("aaaa1111", "Deep Residual Learning", PaperStatus::Resolved),
    )
    .await
    .unwrap();
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();

    let tag: serde_json::Value = server
        .put("/api/papers/aaaa1111/tags")
        .json(&serde_json::json!({"name": "nlp"}))
        .await
        .json();
    let tag_id = tag["id"].as_str().unwrap().to_string();

    // Rename -> 200, name changed.
    let resp = server
        .patch(&format!("/api/tags/{tag_id}"))
        .json(&serde_json::json!({"name": "natural-language-processing"}))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["name"], "natural-language-processing");

    // Rename an unknown id -> 404.
    server
        .patch("/api/tags/does-not-exist")
        .json(&serde_json::json!({"name": "whatever"}))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // Delete -> 204, then 404 when already gone.
    server
        .delete(&format!("/api/tags/{tag_id}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    server
        .delete(&format!("/api/tags/{tag_id}"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tag_names_that_normalize_to_empty_are_bad_requests() {
    let (dir, pool) = support::temp_pool().await;
    papers::insert(
        &pool,
        &paper("aaaa1111", "Deep Residual Learning", PaperStatus::Resolved),
    )
    .await
    .unwrap();
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();

    let blank = server
        .put("/api/papers/aaaa1111/tags")
        .json(&serde_json::json!({"name": "   "}))
        .await;
    blank.assert_status(axum::http::StatusCode::BAD_REQUEST);
    let blank_body: serde_json::Value = blank.json();
    assert_eq!(blank_body, serde_json::json!({"error": "empty name"}));

    for name in ["///", " / / "] {
        let resp = server
            .put("/api/papers/aaaa1111/tags")
            .json(&serde_json::json!({ "name": name }))
            .await;
        resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(resp.json::<serde_json::Value>(), blank_body);
    }

    let tag: serde_json::Value = server
        .put("/api/papers/aaaa1111/tags")
        .json(&serde_json::json!({"name": "nlp"}))
        .await
        .json();
    let resp = server
        .patch(&format!("/api/tags/{}", tag["id"].as_str().unwrap()))
        .json(&serde_json::json!({"name": "///"}))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(resp.json::<serde_json::Value>(), blank_body);

    let list: Vec<serde_json::Value> = server.get("/api/tags").await.json();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["name"], "nlp");
}

#[tokio::test]
async fn star_and_unstar_paper() {
    let (dir, pool) = support::temp_pool().await;
    papers::insert(
        &pool,
        &paper("aaaa1111", "Deep Residual Learning", PaperStatus::Resolved),
    )
    .await
    .unwrap();
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();

    // Star -> 204, detail reflects starred:true.
    server
        .put("/api/papers/aaaa1111/star")
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    let detail: serde_json::Value = server.get("/api/papers/aaaa1111").await.json();
    assert_eq!(detail["starred"], true);

    // Unstar -> 204, detail reflects starred:false.
    server
        .delete("/api/papers/aaaa1111/star")
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    let detail: serde_json::Value = server.get("/api/papers/aaaa1111").await.json();
    assert_eq!(detail["starred"], false);

    // Starring a missing paper -> 404.
    server
        .put("/api/papers/nope/star")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn set_and_clear_paper_name() {
    let (dir, pool) = support::temp_pool().await;
    papers::insert(
        &pool,
        &paper("aaaa1111", "Deep Residual Learning", PaperStatus::Resolved),
    )
    .await
    .unwrap();
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();

    // Set -> 200 echoing the trimmed value; detail reflects it.
    let res = server
        .patch("/api/papers/aaaa1111/name")
        .json(&serde_json::json!({ "name": "  ResNet  " }));
    let res = res.await;
    res.assert_status(axum::http::StatusCode::OK);
    assert_eq!(res.json::<serde_json::Value>()["name"], "ResNet");
    let detail: serde_json::Value = server.get("/api/papers/aaaa1111").await.json();
    assert_eq!(detail["name"], "ResNet");

    // Whitespace-only clears back to null.
    let res = server
        .patch("/api/papers/aaaa1111/name")
        .json(&serde_json::json!({ "name": "   " }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    assert_eq!(
        res.json::<serde_json::Value>()["name"],
        serde_json::Value::Null
    );
    let detail: serde_json::Value = server.get("/api/papers/aaaa1111").await.json();
    assert_eq!(detail["name"], serde_json::Value::Null);

    // Over-length and control characters -> 400; missing paper -> 404.
    server
        .patch("/api/papers/aaaa1111/name")
        .json(&serde_json::json!({ "name": "x".repeat(201) }))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
    server
        .patch("/api/papers/aaaa1111/name")
        .json(&serde_json::json!({ "name": "two\nlines" }))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
    server
        .patch("/api/papers/nope/name")
        .json(&serde_json::json!({ "name": "X" }))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn import_url_needs_ingest_context() {
    let (dir, pool) = support::temp_pool().await;
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();
    server
        .post("/api/import")
        .json(&serde_json::json!({ "input": "10.1145/x" }))
        .await
        .assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn exports_bibtex_and_biblatex() {
    let (dir, pool) = support::temp_pool().await;
    papers::insert(
        &pool,
        &paper("aaaa1111", "Deep Residual Learning", PaperStatus::Resolved),
    )
    .await
    .unwrap();
    papers::insert(
        &pool,
        &paper(
            "bbbb2222",
            "Attention Is All You Need",
            PaperStatus::Resolved,
        ),
    )
    .await
    .unwrap();
    let proj = projects::create(&pool, &ProjectName::parse("Survey").unwrap())
        .await
        .unwrap();
    projects::add_paper(&pool, "aaaa1111", &proj.id)
        .await
        .unwrap();
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();

    // Individual (default bibtex). The `paper` helper sets venue=KDD, no dblp_key -> @article.
    let resp = server.get("/api/papers/aaaa1111/export").await;
    resp.assert_status_ok();
    assert_eq!(
        resp.header("content-type"),
        "text/plain; charset=utf-8",
        "single-entry export should be inline plain text"
    );
    let text = resp.text();
    assert!(text.contains("@article{aaaa1111,"), "got: {text}");
    assert!(text.contains("journal = {KDD},"));

    // BibLaTeX switches the field names.
    let bl = server
        .get("/api/papers/aaaa1111/export?format=biblatex")
        .await
        .text();
    assert!(bl.contains("journaltitle = {KDD},"), "got: {bl}");
    assert!(bl.contains("date = {2020},"));

    // Unknown id -> 404.
    server
        .get("/api/papers/nope/export")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // Batch: whole library has both entries, served as a downloadable .bib file.
    let all = server.get("/api/papers/export").await;
    all.assert_status_ok();
    assert_eq!(all.header("content-type"), "application/x-bibtex");
    let disposition = all.header("content-disposition");
    let disposition = disposition.to_str().unwrap();
    assert!(disposition.contains("attachment"), "got: {disposition}");
    assert!(disposition.contains("xuewen.bib"), "got: {disposition}");
    let all_text = all.text();
    assert!(all_text.contains("@article{aaaa1111,"));
    assert!(all_text.contains("@article{bbbb2222,"));

    // Batch filtered by project -> only that project's paper.
    let scoped = server
        .get(&format!("/api/papers/export?project={}", proj.id))
        .await
        .text();
    assert!(scoped.contains("aaaa1111"));
    assert!(!scoped.contains("bbbb2222"));

    // format + filter combine: the handler extracts the same query string
    // twice (FormatParam and the list endpoint's ListParams).
    let scoped_bl = server
        .get(&format!(
            "/api/papers/export?format=biblatex&project={}",
            proj.id
        ))
        .await
        .text();
    assert!(
        scoped_bl.contains("journaltitle = {KDD},"),
        "got: {scoped_bl}"
    );
    assert!(!scoped_bl.contains("bbbb2222"));
}

#[tokio::test]
async fn paper_rows_carry_starred_tags_and_projects() {
    let (dir, pool) = support::temp_pool().await;
    papers::insert(
        &pool,
        &paper("aaaa1111", "Deep Residual Learning", PaperStatus::Resolved),
    )
    .await
    .unwrap();
    papers::set_starred(&pool, "aaaa1111", true).await.unwrap();
    let tag = tags::add_paper(&pool, "aaaa1111", &TagName::parse("vision").unwrap())
        .await
        .unwrap();
    let proj = projects::create(&pool, &ProjectName::parse("Survey").unwrap())
        .await
        .unwrap();
    projects::add_paper(&pool, "aaaa1111", &proj.id)
        .await
        .unwrap();
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();

    // List row.
    let list: Vec<serde_json::Value> = server.get("/api/papers").await.json();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["starred"], true);
    assert_eq!(
        list[0]["tags"],
        serde_json::json!([{"id": tag.id, "name": "vision"}])
    );
    assert_eq!(
        list[0]["projects"],
        serde_json::json!([{"id": proj.id, "name": "Survey"}])
    );

    // Detail.
    let detail: serde_json::Value = server.get("/api/papers/aaaa1111").await.json();
    assert_eq!(detail["starred"], true);
    assert_eq!(
        detail["tags"],
        serde_json::json!([{"id": tag.id, "name": "vision"}])
    );
    assert_eq!(
        detail["projects"],
        serde_json::json!([{"id": proj.id, "name": "Survey"}])
    );
}

#[tokio::test]
async fn list_filters_by_tag_prefix_and_starred() {
    let (dir, pool) = support::temp_pool().await;
    papers::insert(
        &pool,
        &paper("aaaa1111", "Deep Residual Learning", PaperStatus::Resolved),
    )
    .await
    .unwrap();
    papers::insert(
        &pool,
        &paper(
            "bbbb2222",
            "Attention Is All You Need",
            PaperStatus::Resolved,
        ),
    )
    .await
    .unwrap();
    tags::add_paper(
        &pool,
        "aaaa1111",
        &TagName::parse("security/fuzzing").unwrap(),
    )
    .await
    .unwrap();
    papers::set_starred(&pool, "bbbb2222", true).await.unwrap();
    let proj = projects::create(&pool, &ProjectName::parse("Survey").unwrap())
        .await
        .unwrap();
    projects::add_paper(&pool, "aaaa1111", &proj.id)
        .await
        .unwrap();
    let server = TestServer::new(support::router(pool, dir.path().join("library"))).unwrap();

    // ?tag= matches by prefix -> only the tagged paper.
    let by_tag: Vec<serde_json::Value> = server.get("/api/papers?tag=security").await.json();
    assert_eq!(by_tag.len(), 1);
    assert_eq!(by_tag[0]["id"], "aaaa1111");

    // ?starred=true -> only the starred paper (serde parses the bool).
    let starred: Vec<serde_json::Value> = server.get("/api/papers?starred=true").await.json();
    assert_eq!(starred.len(), 1);
    assert_eq!(starred[0]["id"], "bbbb2222");

    // ?project= still works unchanged.
    let by_project: Vec<serde_json::Value> = server
        .get(&format!("/api/papers?project={}", proj.id))
        .await
        .json();
    assert_eq!(by_project.len(), 1);
    assert_eq!(by_project[0]["id"], "aaaa1111");
}

mod search_api {
    use super::*; // reuse the file's pool/server helpers
    use xuewen::search::{fts, SearchService};

    // This file's pool/paper helpers are named `support::temp_pool`/`paper`
    // (not `test_pool`/`insert_sample_paper` as in the brief); these two small
    // wrappers adapt to that existing local style.
    async fn test_pool() -> sqlx::SqlitePool {
        let (dir, pool) = support::temp_pool().await;
        std::mem::forget(dir); // keep the sqlite file alive for the test's duration
        pool
    }

    async fn insert_sample_paper(pool: &sqlx::SqlitePool, id: &str, title: &str) {
        papers::insert(pool, &paper(id, title, PaperStatus::Resolved))
            .await
            .unwrap();
    }

    async fn server_with_search(pool: sqlx::SqlitePool) -> axum_test::TestServer {
        let idx = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(idx.path()).unwrap();
        std::mem::forget(idx);
        let svc = SearchService::open_with(pool.clone(), fts_idx, None);
        svc.fts
            .upsert(&fts::PaperDoc {
                id: "p1".into(),
                title: "Fuzzing Firmware".into(),
                authors: "Ada Lovelace".into(),
                venue: String::new(),
                abstract_text: String::new(),
                body: "router dictionaries".into(),
                notes: String::new(),
            })
            .unwrap();
        let app = xuewen::app::App::builder(pool, std::path::PathBuf::from("/nonexistent"))
            .search(svc)
            .build();
        axum_test::TestServer::new(xuewen::web::router(app)).unwrap()
    }

    #[tokio::test]
    async fn search_returns_papers_with_match_info() {
        let pool = test_pool().await; // the file's existing helper
        insert_sample_paper(&pool, "p1", "Fuzzing Firmware").await; // existing helper or add one
        let server = server_with_search(pool).await;

        let resp = server
            .get("/api/search")
            .add_query_param("q", "fuzzing")
            .await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert_eq!(body["semantic"]["available"], false);
        assert_eq!(body["results"][0]["paper"]["id"], "p1");
        assert_eq!(body["results"][0]["match"]["engine"], "keyword");
        assert!(body["results"][0]["match"]["snippet"]
            .as_str()
            .unwrap()
            .contains("<mark>"));
    }

    #[tokio::test]
    async fn fields_param_restricts_search() {
        let pool = test_pool().await;
        insert_sample_paper(&pool, "p1", "Fuzzing Firmware").await;
        let server = server_with_search(pool).await;
        let resp = server
            .get("/api/search")
            .add_query_param("q", "dictionaries")
            .add_query_param("fields", "title")
            .await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert_eq!(body["results"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn status_reports_tiers() {
        let pool = test_pool().await;
        insert_sample_paper(&pool, "p1", "Fuzzing Firmware").await;
        let server = server_with_search(pool).await;
        let resp = server.get("/api/search/status").await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert!(body["fts"]["pending"].as_i64().unwrap() >= 1);
        assert_eq!(body["semantic_available"], false);
    }

    /// Two papers both matching "fuzzing" in FTS: "a" (tag nlp, starred, in
    /// project "Thesis"), "b" (tag systems, no star, no project).
    async fn server_with_qualifier_fixtures(pool: sqlx::SqlitePool) -> axum_test::TestServer {
        insert_sample_paper(&pool, "a", "Fuzzing Firmware").await;
        insert_sample_paper(&pool, "b", "Fuzzing Kernels").await;
        tags::add_paper(&pool, "a", &TagName::parse("nlp").unwrap())
            .await
            .unwrap();
        tags::add_paper(&pool, "b", &TagName::parse("systems").unwrap())
            .await
            .unwrap();
        papers::set_starred(&pool, "a", true).await.unwrap();
        let proj = projects::create(&pool, &ProjectName::parse("Thesis").unwrap())
            .await
            .unwrap();
        projects::add_paper(&pool, "a", &proj.id).await.unwrap();

        let idx = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(idx.path()).unwrap();
        std::mem::forget(idx);
        let svc = SearchService::open_with(pool.clone(), fts_idx, None);
        for (id, title) in [("a", "Fuzzing Firmware"), ("b", "Fuzzing Kernels")] {
            svc.fts
                .upsert(&fts::PaperDoc {
                    id: id.into(),
                    title: title.into(),
                    authors: "Ada Lovelace".into(),
                    venue: String::new(),
                    abstract_text: String::new(),
                    body: String::new(),
                    notes: String::new(),
                })
                .unwrap();
        }
        let app = xuewen::app::App::builder(pool, std::path::PathBuf::from("/nonexistent"))
            .search(svc)
            .build();
        axum_test::TestServer::new(xuewen::web::router(app)).unwrap()
    }

    #[tokio::test]
    async fn search_query_syntax_filters_by_tag_and_star() {
        let pool = test_pool().await;
        let server = server_with_qualifier_fixtures(pool).await;
        let resp = server
            .get("/api/search")
            .add_query_param("q", "fuzzing tag:nlp is:starred")
            .await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        let results = body["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["paper"]["id"], "a");
    }

    #[tokio::test]
    async fn search_project_qualifier_resolves_name_case_insensitively() {
        let pool = test_pool().await;
        let server = server_with_qualifier_fixtures(pool).await;
        let resp = server
            .get("/api/search")
            .add_query_param("q", "fuzzing project:thesis")
            .await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        let results = body["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["paper"]["id"], "a");

        let resp = server
            .get("/api/search")
            .add_query_param("q", "fuzzing project:nosuch")
            .await;
        let body: serde_json::Value = resp.json();
        assert!(
            body["results"].as_array().unwrap().is_empty(),
            "unknown project name → empty"
        );
    }

    #[tokio::test]
    async fn search_qualifiers_override_explicit_params() {
        let pool = test_pool().await;
        let server = server_with_qualifier_fixtures(pool).await;
        let resp = server
            .get("/api/search")
            .add_query_param("q", "fuzzing tag:nlp")
            .add_query_param("tag", "systems")
            .await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        let results = body["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0]["paper"]["id"], "a",
            "tag: qualifier must beat ?tag="
        );
    }

    #[tokio::test]
    async fn search_results_include_tags_and_projects() {
        let pool = test_pool().await;
        insert_sample_paper(&pool, "p1", "Fuzzing Firmware").await;
        tags::add_paper(&pool, "p1", &TagName::parse("security/fuzzing").unwrap())
            .await
            .unwrap();
        let pr = projects::create(&pool, &ProjectName::parse("TP3").unwrap())
            .await
            .unwrap();
        projects::add_paper(&pool, "p1", &pr.id).await.unwrap();
        let server = server_with_search(pool).await;

        let resp = server
            .get("/api/search")
            .add_query_param("q", "fuzzing")
            .await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        let paper = &body["results"][0]["paper"];
        assert_eq!(paper["tags"][0]["name"], "security/fuzzing");
        assert_eq!(paper["projects"][0]["name"], "TP3");
    }

    #[tokio::test]
    async fn search_without_service_is_503() {
        let pool = test_pool().await;
        let router = support::router(pool, std::path::PathBuf::from("/nonexistent"));
        let server = axum_test::TestServer::new(router).unwrap();
        let resp = server.get("/api/search").add_query_param("q", "x").await;
        resp.assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
    }
}
