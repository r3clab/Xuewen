use crate::support;

use wiremock::matchers::{method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xuewen::db;
use xuewen::ingest::pipeline::{Disposal, IngestCtx, Libraries, Outcome};
use xuewen::library::model::{Authors, Identifier, PaperMeta, PaperStatus};
use xuewen::library::{papers, trash};
use xuewen::metadata::resolve::grobid::Grobid;
use xuewen::metadata::resolve::Resolver;

const TEI_FIXTURE: &str = support::fixture!("grobid_bert.tei.xml");
const DBLP_BERT_FIXTURE: &str = support::fixture!("dblp_bert.json");

#[tokio::test]
async fn ingests_pdf_and_dedups() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    // A PDF whose header carries a title and a DOI.
    let pdf_path = inbox.join("paper.pdf");
    support::write_test_pdf(
        &pdf_path,
        &[
            "Attention Is All You Need",
            "https://doi.org/10.1145/3292500.3330701",
        ],
    );

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let mock = MockServer::start().await;
    let resolver = Resolver::with_bases(None, mock.uri(), mock.uri()).unwrap();
    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(&inbox, &library),
        resolver,
        None,
    );

    // First ingest: stored, filed, moved.
    let out = ctx.ingest_file(&pdf_path).await.unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        other => panic!("expected Ingested, got {other:?}"),
    };

    let paper = papers::get(&pool, &id).await.unwrap().unwrap();
    assert_eq!(
        paper.meta.title.as_deref(),
        Some("Attention Is All You Need")
    );
    assert_eq!(paper.meta.doi.as_deref(), Some("10.1145/3292500.3330701"));
    assert_eq!(paper.meta.status, PaperStatus::NeedsReview);

    // File was copied into the library and the original moved to _processed.
    assert!(library
        .join(format!("_unsorted/{}.pdf", paper.content_hash))
        .exists());
    assert_eq!(paper.cite_key, None);
    assert!(!pdf_path.exists());
    assert!(processed.join("paper.pdf").exists());

    // Re-ingest identical content (from processed copy) -> Duplicate carrying
    // the existing paper's id; the already-archived file stays put.
    let again = processed.join("paper.pdf");
    let out2 = ctx.ingest_file(&again).await.unwrap();
    assert_eq!(out2, Outcome::Duplicate(id));
    assert!(again.exists());
}

#[tokio::test]
async fn same_doi_different_bytes_reports_same_work() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    let doi_line = "https://doi.org/10.1000/xyz123";
    let a = inbox.join("a.pdf");
    let b = inbox.join("b.pdf");
    support::write_test_pdf(&a, &["Paper A Title", doi_line]);
    support::write_test_pdf(&b, &["Paper B Different Title", doi_line]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let mock = MockServer::start().await;
    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(&inbox, &library),
        Resolver::with_bases(None, mock.uri(), mock.uri()).unwrap(),
        None,
    );

    let id_a = match ctx.ingest_file(&a).await.unwrap() {
        Outcome::Ingested(id) => id,
        other => panic!("expected Ingested, got {other:?}"),
    };

    // Same DOI, different bytes → reported as the same work; file archived.
    let out = ctx.ingest_file(&b).await.unwrap();
    assert_eq!(out, Outcome::SameWork(id_a));
    assert_eq!(
        std::fs::read_dir(library.join("_unsorted"))
            .unwrap()
            .count(),
        1
    );
    assert!(!b.exists());
    assert!(processed.join("b.pdf").exists());
}

const CROSSREF_FIXTURE: &str = support::fixture!("crossref_kgat.json");
const ARXIV_FIXTURE: &str = support::fixture!("arxiv_attention.xml");

#[tokio::test]
async fn ingest_with_doi_resolves_via_crossref() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();

    let doi = "10.1145/3292500.3330701";
    let pdf_path = inbox.join("paper.pdf");
    support::write_test_pdf(
        &pdf_path,
        &["Some Provisional Header", &format!("https://doi.org/{doi}")],
    );

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(&inbox, &library),
        resolver,
        None,
    );

    let out = ctx.ingest_file(&pdf_path).await.unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        other => panic!("expected Ingested, got {other:?}"),
    };

    let paper = papers::get(&pool, &id).await.unwrap().unwrap();
    assert_eq!(paper.meta.status, PaperStatus::Resolved);
    assert_eq!(paper.meta.source.as_deref(), Some("crossref"));
    assert_eq!(
        paper.meta.title.as_deref(),
        Some("KGAT: Knowledge Graph Attention Network for Recommendation")
    );
    assert_eq!(paper.meta.doi.as_deref(), Some(doi));
    assert_eq!(paper.meta.year, Some(2019));
    assert!(paper.meta.authors.0.iter().any(|a| a == "Xiang Wang"));
    assert_eq!(paper.cite_key.as_deref(), Some("wang2019kgat"));
    assert!(library.join("wang2019kgat.pdf").exists());
}

#[tokio::test]
async fn ingest_with_arxiv_resolves_via_api() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();

    let arxiv_id = "1706.03762";
    let pdf_path = inbox.join("paper.pdf");
    support::write_test_pdf(
        &pdf_path,
        &["Provisional Header", &format!("arXiv:{arxiv_id}")],
    );

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/api/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ARXIV_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(&inbox, &library),
        resolver,
        None,
    );

    let out = ctx.ingest_file(&pdf_path).await.unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        other => panic!("expected Ingested, got {other:?}"),
    };

    let paper = papers::get(&pool, &id).await.unwrap().unwrap();
    assert_eq!(paper.meta.status, PaperStatus::Resolved);
    assert_eq!(paper.meta.source.as_deref(), Some("arxiv"));
    assert_eq!(
        paper.meta.title.as_deref(),
        Some("Attention Is All You Need")
    );
    assert_eq!(paper.meta.arxiv_id.as_deref(), Some(arxiv_id));
    assert_eq!(paper.meta.year, Some(2017));
    assert_eq!(paper.cite_key.as_deref(), Some("vaswani2017attention"));
    assert!(library.join("vaswani2017attention.pdf").exists());
}

const DBLP_FIXTURE: &str = support::fixture!("dblp_kgat.json");

#[tokio::test]
async fn ingest_without_identifier_resolves_via_dblp() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();

    // No DOI/arXiv anywhere; the first substantive line is the title. The
    // author line is load-bearing: a title-only match is confirmed against the
    // paper's own text, so a page naming nobody resolves to nothing.
    let pdf_path = inbox.join("paper.pdf");
    support::write_test_pdf(
        &pdf_path,
        &[
            "KGAT: Knowledge Graph Attention Network for Recommendation",
            "Xiang Wang, Xiangnan He, Yixin Cao",
        ],
    );

    // DBLP mock returns the matching hit.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DBLP_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(&inbox, &library),
        resolver,
        None,
    );

    let out = ctx.ingest_file(&pdf_path).await.unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        other => panic!("expected Ingested, got {other:?}"),
    };

    let paper = papers::get(&pool, &id).await.unwrap().unwrap();
    assert_eq!(paper.meta.status, PaperStatus::Resolved);
    assert_eq!(paper.meta.source.as_deref(), Some("dblp"));
    assert_eq!(paper.meta.dblp_key.as_deref(), Some("conf/kdd/WangHCLC19"));
    assert_eq!(paper.meta.venue.as_deref(), Some("KDD"));
    assert_eq!(paper.meta.year, Some(2019));
    assert!(paper.meta.doi.as_deref().is_some());
    assert_eq!(paper.cite_key.as_deref(), Some("wang2019kgat"));
    assert!(library.join("wang2019kgat.pdf").exists());
}

#[tokio::test]
async fn grobid_title_drives_dblp_resolution() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();

    // The PDF's own text is a poor/truncated title; GROBID supplies the clean
    // one. The authors still extract, and are what confirms DBLP's record.
    let pdf_path = inbox.join("paper.pdf");
    support::write_test_pdf(
        &pdf_path,
        &[
            "BERT Pre-training of Deep Bidir",
            "Jacob Devlin, Ming-Wei Chang, Kenton Lee, Kristina Toutanova",
        ],
    );

    // GROBID returns the full BERT header; DBLP is stubbed with the BERT record.
    let grobid_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(wm_path("/api/processHeaderDocument"))
        .respond_with(ResponseTemplate::new(200).set_body_string(TEI_FIXTURE))
        .mount(&grobid_server)
        .await;
    let grobid = Grobid::new(&grobid_server.uri()).unwrap();

    let api_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DBLP_BERT_FIXTURE))
        .mount(&api_server)
        .await;
    let resolver = Resolver::with_bases(None, api_server.uri(), api_server.uri())
        .unwrap()
        .with_dblp_base(api_server.uri());

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(&inbox, &library),
        resolver,
        Some(grobid),
    );

    let out = ctx.ingest_file(&pdf_path).await.unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        other => panic!("expected Ingested, got {other:?}"),
    };
    let paper = papers::get(&pool, &id).await.unwrap().unwrap();
    // DBLP matched (its title fuzzily matches the GROBID title) -> resolved via dblp.
    assert_eq!(paper.meta.status, PaperStatus::Resolved);
    assert_eq!(paper.meta.source.as_deref(), Some("dblp"));
    // DBLP has no abstract; the GROBID abstract is backfilled.
    assert!(paper
        .meta
        .abstract_text
        .as_deref()
        .unwrap()
        .contains("language representation model"));
}

#[tokio::test]
async fn grobid_enriches_needs_review_when_unmatched() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();

    let pdf_path = inbox.join("paper.pdf");
    support::write_test_pdf(&pdf_path, &["garbled first line xyz"]);

    let grobid_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(wm_path("/api/processHeaderDocument"))
        .respond_with(ResponseTemplate::new(200).set_body_string(TEI_FIXTURE))
        .mount(&grobid_server)
        .await;
    let grobid = Grobid::new(&grobid_server.uri()).unwrap();

    // Resolver points at a stub-less server: DBLP + Crossref both 404 -> Unresolved.
    let api_server = MockServer::start().await;
    let resolver = Resolver::with_bases(None, api_server.uri(), api_server.uri())
        .unwrap()
        .with_dblp_base(api_server.uri());

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(&inbox, &library),
        resolver,
        Some(grobid),
    );

    let out = ctx.ingest_file(&pdf_path).await.unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        other => panic!("expected Ingested, got {other:?}"),
    };
    let paper = papers::get(&pool, &id).await.unwrap().unwrap();
    assert_eq!(paper.meta.status, PaperStatus::NeedsReview);
    assert_eq!(paper.meta.source.as_deref(), Some("grobid"));
    assert_eq!(
        paper.meta.title.as_deref(),
        Some("BERT: Pre-training of Deep Bidirectional Transformers for Language Understanding")
    );
    assert!(paper.meta.authors.0.iter().any(|a| a == "Jacob Devlin"));
}

#[tokio::test]
async fn colliding_cite_key_gets_letter_suffix() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();

    let doi = "10.1145/3292500.3330701";
    let pdf_path = inbox.join("paper.pdf");
    support::write_test_pdf(&pdf_path, &["Header", &format!("https://doi.org/{doi}")]);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();

    // Pre-seed a different paper that already owns the base key "wang2019kgat".
    let seed = xuewen::library::model::Paper {
        id: "01890000-0000-7000-8000-0000000000ff".to_string(),
        content_hash: "seedhash".to_string(),
        rel_path: "wang2019kgat.pdf".to_string(),
        cite_key: Some("wang2019kgat".to_string()),
        added_at: "2026-07-07T00:00:00Z".to_string(),
        deleted_at: None,
        starred: false,
        name: None,
        meta: PaperMeta {
            title: Some("Seed".to_string()),
            abstract_text: None,
            authors: Authors::default(),
            venue: None,
            year: Some(2019),
            doi: None,
            arxiv_id: None,
            dblp_key: None,
            url: None,
            source: Some("crossref".to_string()),
            status: PaperStatus::Resolved,
        },
    };
    papers::insert(&pool, &seed).await.unwrap();

    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(&inbox, &library),
        resolver,
        None,
    );
    let out = ctx.ingest_file(&pdf_path).await.unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        other => panic!("expected Ingested, got {other:?}"),
    };
    let paper = papers::get(&pool, &id).await.unwrap().unwrap();
    assert_eq!(paper.cite_key.as_deref(), Some("wang2019kgata"));
    assert!(library.join("wang2019kgata.pdf").exists());
}

#[tokio::test]
async fn citekey_collision_on_disk_only_picks_the_next_suffix() {
    // A concurrent ingest copies its PDF to the cite-key path BEFORE inserting
    // its row. Simulate having lost that race: the disambiguated path exists
    // on disk with no DB row claiming its key. The loser must not overwrite it
    // (create_new) and must advance past it via its local taken set — the DB
    // alone would keep handing back the same "free" key.
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    std::fs::create_dir_all(&library).unwrap();

    let doi = "10.1145/3292500.3330701";
    let pdf_path = inbox.join("paper.pdf");
    support::write_test_pdf(&pdf_path, &["Header", &format!("https://doi.org/{doi}")]);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    // The DB knows about "wang2019kgat"…
    let seed = xuewen::library::model::Paper {
        id: "01890000-0000-7000-8000-0000000000ff".to_string(),
        content_hash: "seedhash".to_string(),
        rel_path: "wang2019kgat.pdf".to_string(),
        cite_key: Some("wang2019kgat".to_string()),
        added_at: "2026-07-07T00:00:00Z".to_string(),
        deleted_at: None,
        starred: false,
        name: None,
        meta: PaperMeta {
            title: Some("Seed".to_string()),
            abstract_text: None,
            authors: Authors::default(),
            venue: None,
            year: Some(2019),
            doi: None,
            arxiv_id: None,
            dblp_key: None,
            url: None,
            source: Some("crossref".to_string()),
            status: PaperStatus::Resolved,
        },
    };
    papers::insert(&pool, &seed).await.unwrap();
    // …while "wang2019kgata.pdf" exists only on disk (the racing winner's copy).
    std::fs::write(library.join("wang2019kgata.pdf"), b"winner bytes").unwrap();

    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(&inbox, &library),
        Resolver::with_bases(None, server.uri(), server.uri()).unwrap(),
        None,
    );
    let id = match ctx.ingest_file(&pdf_path).await.unwrap() {
        Outcome::Ingested(id) => id,
        other => panic!("expected Ingested, got {other:?}"),
    };
    let paper = papers::get(&pool, &id).await.unwrap().unwrap();
    assert_eq!(paper.cite_key.as_deref(), Some("wang2019kgatb"));
    assert!(library.join("wang2019kgatb.pdf").exists());
    assert_eq!(
        std::fs::read(library.join("wang2019kgata.pdf")).unwrap(),
        b"winner bytes",
        "the racing winner's file must not be overwritten"
    );
}

#[tokio::test]
async fn ingest_bytes_discards_the_staged_copy_on_success() {
    // Uploads and URL imports feed ingest an app-created staged copy whose
    // bytes step 5 files into the library; nothing of it belongs in
    // `_processed` (that would store every import twice forever).
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let src = dir.path().join("fetched.pdf");
    support::write_test_pdf(&src, &["A Paper Imported From A Url"]);
    let bytes = std::fs::read(&src).unwrap();

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(&inbox, &library),
        Resolver::with_bases(
            None,
            "http://127.0.0.1:1".to_string(),
            "http://127.0.0.1:1".to_string(),
        )
        .unwrap()
        .with_dblp_base("http://127.0.0.1:1".to_string()),
        None,
    );

    let out = ctx.ingest_bytes(&bytes, "import.pdf", None).await.unwrap();
    assert!(matches!(out, Outcome::Ingested(_)), "got {out:?}");
    assert_eq!(
        std::fs::read_dir(inbox.join("_uploads")).unwrap().count(),
        0,
        "staged copy must be deleted once ingested"
    );
    assert!(
        !inbox.join("_processed").exists(),
        "imports must not be archived to _processed"
    );
    // The library holds the one and only copy.
    assert_eq!(
        std::fs::read_dir(library.join("_unsorted"))
            .unwrap()
            .count(),
        1
    );
}

#[tokio::test]
async fn reingesting_a_trashed_paper_reports_in_trash() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    let pdf_path = inbox.join("paper.pdf");
    support::write_test_pdf(
        &pdf_path,
        &[
            "Attention Is All You Need",
            "https://doi.org/10.1145/3292500.3330701",
        ],
    );

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let mock = MockServer::start().await;
    let resolver = Resolver::with_bases(None, mock.uri(), mock.uri()).unwrap();
    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(&inbox, &library),
        resolver,
        None,
    );

    let out = ctx.ingest_file(&pdf_path).await.unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        other => panic!("expected Ingested, got {other:?}"),
    };
    trash::soft_delete(&pool, &id).await.unwrap();

    // Dedup is by content_hash, and the trashed row still holds it → InTrash.
    let again = processed.join("paper.pdf");
    let out2 = ctx.ingest_file(&again).await.unwrap();
    assert_eq!(out2, Outcome::InTrash(id));
}

#[tokio::test]
async fn same_doi_of_trashed_paper_reports_in_trash() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();

    let doi_line = "https://doi.org/10.1000/xyz123";
    let a = inbox.join("a.pdf");
    let b = inbox.join("b.pdf");
    support::write_test_pdf(&a, &["Paper A Title", doi_line]);
    support::write_test_pdf(&b, &["Paper B Different Title", doi_line]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let mock = MockServer::start().await;
    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(&inbox, &library),
        Resolver::with_bases(None, mock.uri(), mock.uri()).unwrap(),
        None,
    );

    let id_a = match ctx.ingest_file(&a).await.unwrap() {
        Outcome::Ingested(id) => id,
        other => panic!("expected Ingested, got {other:?}"),
    };
    trash::soft_delete(&pool, &id_a).await.unwrap();

    let out = ctx.ingest_file(&b).await.unwrap();
    assert_eq!(out, Outcome::InTrash(id_a));
}

const DBLP_ANTIFUZZ_FIXTURE: &str = support::fixture!("dblp_antifuzz.json");

#[tokio::test]
async fn wrapped_two_line_title_resolves_via_dblp() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();

    // USENIX-style cover sheet: the title wraps across two lines, no DOI.
    let pdf_path = inbox.join("paper.pdf");
    support::write_test_pdf(
        &pdf_path,
        &[
            "AntiFuzz: Impeding Fuzzing Audits of",
            "Binary Executables",
            "Emre Guler, Cornelius Aschermann, Ali Abbasi, and Thorsten Holz,",
            "Ruhr-Universitat Bochum",
        ],
    );

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DBLP_ANTIFUZZ_FIXTURE))
        .mount(&server)
        .await;
    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(&inbox, &library),
        Resolver::with_bases(None, server.uri(), server.uri())
            .unwrap()
            .with_dblp_base(server.uri()),
        None,
    );

    let id = match ctx.ingest_file(&pdf_path).await.unwrap() {
        Outcome::Ingested(id) => id,
        other => panic!("expected Ingested, got {other:?}"),
    };
    let paper = papers::get(&pool, &id).await.unwrap().unwrap();
    // The joined title matches DBLP's record at similarity 1.0 -> auto-resolved.
    assert_eq!(paper.meta.status, PaperStatus::Resolved);
    assert_eq!(paper.meta.source.as_deref(), Some("dblp"));
    assert_eq!(paper.cite_key.as_deref(), Some("guler2019antifuzz"));
    assert!(library.join("guler2019antifuzz.pdf").exists());
}

#[tokio::test]
async fn ingest_with_hint_seeds_identifier() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();

    // Offline resolver: upstreams refuse instantly (degrades to needs_review).
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string());
    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(&inbox, &library),
        resolver,
        None,
    );

    // A one-page PDF whose text carries NO identifier.
    let pdf_path = inbox.join("in.pdf");
    support::write_test_pdf(&pdf_path, &["A Paper Without Any Identifier In Text"]);

    // Import with a DOI hint: the hint seeds the stored identifier even though
    // the PDF text has none.
    let id = match ctx
        .ingest_file_with_hint(
            &pdf_path,
            Some(Identifier::Doi("10.1234/hinted".into())),
            Disposal::Archive,
        )
        .await
        .unwrap()
    {
        Outcome::Ingested(id) => id,
        other => panic!("expected Ingested, got {other:?}"),
    };
    let got = papers::get(&pool, &id).await.unwrap().unwrap();
    assert_eq!(got.meta.doi.as_deref(), Some("10.1234/hinted"));
    assert_eq!(got.meta.status, PaperStatus::NeedsReview);
}
