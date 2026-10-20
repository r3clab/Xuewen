use crate::support;

use wiremock::matchers::{method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xuewen::db;
use xuewen::ingest::pipeline::{IngestCtx, Libraries};
use xuewen::ingest::refresh::{self, RefreshTarget};
use xuewen::library::model::{Authors, Paper, PaperMeta, PaperStatus};
use xuewen::library::{papers, trash};
use xuewen::metadata::resolve::Resolver;

const CROSSREF_FIXTURE: &str = support::fixture!("crossref_kgat.json");

/// A minimal stored paper for seeding; callers set the fields they care about.
fn seed_paper(id: &str, hash: &str, rel_path: &str, status: PaperStatus) -> Paper {
    Paper {
        id: id.into(),
        content_hash: hash.into(),
        rel_path: rel_path.into(),
        cite_key: None,
        added_at: "2026-07-07T00:00:00Z".into(),
        deleted_at: None,
        starred: false,
        name: None,
        meta: PaperMeta {
            title: None,
            abstract_text: None,
            authors: Authors::default(),
            venue: None,
            year: None,
            doi: None,
            arxiv_id: None,
            dblp_key: None,
            url: None,
            source: None,
            status,
        },
    }
}

#[tokio::test]
async fn needs_review_reresolves_and_refiles() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    let hash = "deadbeefhash";
    let unsorted = library.join(format!("_unsorted/{hash}.pdf"));
    std::fs::create_dir_all(unsorted.parent().unwrap()).unwrap();

    // The stored PDF carries the DOI so re-resolution can identify it.
    let doi = "10.1145/3292500.3330701";
    support::write_test_pdf(
        &unsorted,
        &["Some Header", &format!("https://doi.org/{doi}")],
    );

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let p = seed_paper(
        "01890000-0000-7000-8000-0000000000a1",
        hash,
        &format!("_unsorted/{hash}.pdf"),
        PaperStatus::NeedsReview,
    );
    papers::insert(&pool, &p).await.unwrap();

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(dir.path(), &library),
        resolver,
        None,
    );
    let summary = refresh::run(&ctx, RefreshTarget::NeedsReview)
        .await
        .unwrap();
    assert_eq!(summary.reresolved, 1);
    assert_eq!(summary.refiled, 1);

    let got = papers::get(&pool, &p.id).await.unwrap().unwrap();
    assert_eq!(got.meta.status, PaperStatus::Resolved);
    assert_eq!(got.cite_key.as_deref(), Some("wang2019kgat"));
    assert_eq!(got.rel_path, "wang2019kgat.pdf");
    assert!(library.join("wang2019kgat.pdf").exists());
    assert!(!unsorted.exists());
}

#[tokio::test]
async fn resolved_paper_refiles_without_reresolving() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    std::fs::create_dir_all(&library).unwrap();
    let hash = "abc123hash";
    let old = library.join(format!("{hash}.pdf"));
    // Content is irrelevant — a resolved paper is not re-resolved under the default target.
    support::write_test_pdf(&old, &["Whatever"]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let mut p = seed_paper(
        "01890000-0000-7000-8000-0000000000b2",
        hash,
        &format!("{hash}.pdf"),
        PaperStatus::Resolved,
    );
    p.meta.title = Some("Deep Residual Learning for Image Recognition".into());
    p.meta.authors = Authors(vec!["Kaiming He".into()]);
    p.meta.year = Some(2016);
    p.meta.source = Some("crossref".into());
    papers::insert(&pool, &p).await.unwrap();

    // Unreachable resolver: a resolved paper must NOT be re-resolved, so no HTTP happens.
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".into(),
        "http://127.0.0.1:1".into(),
    )
    .unwrap();

    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(dir.path(), &library),
        resolver,
        None,
    );
    let summary = refresh::run(&ctx, RefreshTarget::NeedsReview)
        .await
        .unwrap();
    assert_eq!(summary.reresolved, 0);
    assert_eq!(summary.refiled, 1);

    let got = papers::get(&pool, &p.id).await.unwrap().unwrap();
    // Metadata unchanged; only the location moved to the cite-key path.
    assert_eq!(got.meta.status, PaperStatus::Resolved);
    assert_eq!(
        got.meta.title.as_deref(),
        Some("Deep Residual Learning for Image Recognition")
    );
    assert_eq!(got.meta.year, Some(2016));
    assert_eq!(got.cite_key.as_deref(), Some("he2016deep"));
    assert_eq!(got.rel_path, "he2016deep.pdf");
    assert!(library.join("he2016deep.pdf").exists());
    assert!(!old.exists());
}

#[tokio::test]
async fn all_does_not_downgrade_resolved_on_failed_reresolve() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    std::fs::create_dir_all(&library).unwrap();
    let hash = "keepmehash";
    // A resolved paper at its cite-key path; the PDF has NO identifier, so a
    // re-resolve attempt yields Unresolved (title search hits an empty mock → 404).
    let f = library.join("he2016deep.pdf");
    support::write_test_pdf(&f, &["No identifier in this text"]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let mut p = seed_paper(
        "01890000-0000-7000-8000-0000000000f6",
        hash,
        "he2016deep.pdf",
        PaperStatus::Resolved,
    );
    p.meta.title = Some("Deep Residual Learning for Image Recognition".into());
    p.meta.authors = Authors(vec!["Kaiming He".into()]);
    p.meta.year = Some(2016);
    p.cite_key = Some("he2016deep".into());
    p.meta.source = Some("crossref".into());
    papers::insert(&pool, &p).await.unwrap();

    // Reachable mock with no mounts → every lookup 404 → Unresolved.
    let server = MockServer::start().await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(dir.path(), &library),
        resolver,
        None,
    );
    let summary = refresh::run(&ctx, RefreshTarget::All).await.unwrap();
    assert_eq!(summary.reresolved, 0); // downgrade prevented → not counted

    let got = papers::get(&pool, &p.id).await.unwrap().unwrap();
    // Metadata preserved; still resolved; still at its cite-key path.
    assert_eq!(got.meta.status, PaperStatus::Resolved);
    assert_eq!(
        got.meta.title.as_deref(),
        Some("Deep Residual Learning for Image Recognition")
    );
    assert_eq!(got.meta.authors, Authors(vec!["Kaiming He".into()]));
    assert_eq!(got.meta.year, Some(2016));
    assert_eq!(got.cite_key.as_deref(), Some("he2016deep"));
    assert_eq!(got.rel_path, "he2016deep.pdf");
    assert!(f.exists());
}

#[tokio::test]
async fn refresh_by_id_prefix_targets_one() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    std::fs::create_dir_all(&library).unwrap();

    // P1: targeted; its PDF carries a DOI so re-resolution succeeds.
    let h1 = "hash0001";
    let doi = "10.1145/3292500.3330701";
    let f1 = library.join(format!("{h1}.pdf"));
    support::write_test_pdf(&f1, &["Header", &format!("https://doi.org/{doi}")]);
    // P2: not targeted; must be untouched.
    let h2 = "hash0002";
    let f2 = library.join(format!("{h2}.pdf"));
    support::write_test_pdf(&f2, &["Other"]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let p1 = seed_paper(
        "01890000-0000-7000-8000-0000000000a1",
        h1,
        &format!("{h1}.pdf"),
        PaperStatus::NeedsReview,
    );
    papers::insert(&pool, &p1).await.unwrap();
    let mut p2 = seed_paper(
        "01890000-0000-7000-8000-0000000000b2",
        h2,
        &format!("{h2}.pdf"),
        PaperStatus::Resolved,
    );
    p2.meta.title = Some("Some Resolved Paper".into());
    papers::insert(&pool, &p2).await.unwrap();

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(dir.path(), &library),
        resolver,
        None,
    );
    // A prefix unique to P1 (P2's id ends ...0000b2).
    let summary = refresh::run(
        &ctx,
        RefreshTarget::One("01890000-0000-7000-8000-0000000000a".into()),
    )
    .await
    .unwrap();
    assert_eq!(summary.processed, 1);

    let got1 = papers::get(&pool, &p1.id).await.unwrap().unwrap();
    assert_eq!(got1.rel_path, "wang2019kgat.pdf");
    assert_eq!(got1.meta.status, PaperStatus::Resolved);

    // P2 completely untouched.
    let got2 = papers::get(&pool, &p2.id).await.unwrap().unwrap();
    assert_eq!(got2.rel_path, format!("{h2}.pdf"));
    assert_eq!(got2.cite_key, None);
    assert!(f2.exists());
}

#[tokio::test]
async fn all_reresolves_resolved_paper() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    std::fs::create_dir_all(&library).unwrap();
    let hash = "stalehash1";
    let doi = "10.1145/3292500.3330701";
    // The paper currently lives at a stale cite-key path; put the real PDF there.
    let f = library.join("old2000stale.pdf");
    support::write_test_pdf(&f, &["Header", &format!("https://doi.org/{doi}")]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let mut p = seed_paper(
        "01890000-0000-7000-8000-0000000000d4",
        hash,
        "old2000stale.pdf",
        PaperStatus::Resolved,
    );
    p.meta.title = Some("Old Stale Title".into());
    p.meta.authors = Authors(vec!["Old Author".into()]);
    p.meta.year = Some(2000);
    p.cite_key = Some("old2000stale".into());
    papers::insert(&pool, &p).await.unwrap();

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(dir.path(), &library),
        resolver,
        None,
    );
    let summary = refresh::run(&ctx, RefreshTarget::All).await.unwrap();
    assert_eq!(summary.reresolved, 1);

    let got = papers::get(&pool, &p.id).await.unwrap().unwrap();
    // Stale metadata replaced by the freshly-resolved record, and re-filed.
    assert_eq!(
        got.meta.title.as_deref(),
        Some("KGAT: Knowledge Graph Attention Network for Recommendation")
    );
    assert_eq!(got.meta.year, Some(2019));
    assert_eq!(got.rel_path, "wang2019kgat.pdf");
    assert!(library.join("wang2019kgat.pdf").exists());
    assert!(!f.exists());
}

#[tokio::test]
async fn refiles_two_same_base_papers_with_distinct_keys() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    std::fs::create_dir_all(&library).unwrap();

    // Two already-resolved papers with identical author/year/title → the same
    // cite-key base, both still at their hash paths. One refresh pass must give
    // them distinct keys (the second suffixed), proving the sequential
    // commit-then-query disambiguation excludes only self and sees prior writes.
    let h1 = "samebaseA";
    let h2 = "samebaseB";
    let f1 = library.join(format!("{h1}.pdf"));
    let f2 = library.join(format!("{h2}.pdf"));
    support::write_test_pdf(&f1, &["A"]);
    support::write_test_pdf(&f2, &["B"]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();

    let mut p1 = seed_paper(
        "01890000-0000-7000-8000-00000000aa01",
        h1,
        &format!("{h1}.pdf"),
        PaperStatus::Resolved,
    );
    p1.meta.title = Some("Deep Residual Learning for Image Recognition".into());
    p1.meta.authors = Authors(vec!["Kaiming He".into()]);
    p1.meta.year = Some(2016);
    p1.added_at = "2026-07-07T00:00:00Z".into();
    papers::insert(&pool, &p1).await.unwrap();

    let mut p2 = seed_paper(
        "01890000-0000-7000-8000-00000000bb02",
        h2,
        &format!("{h2}.pdf"),
        PaperStatus::Resolved,
    );
    p2.meta.title = Some("Deep Residual Learning for Image Recognition".into());
    p2.meta.authors = Authors(vec!["Kaiming He".into()]);
    p2.meta.year = Some(2016);
    p2.added_at = "2026-07-07T00:00:01Z".into(); // ordered after p1
    papers::insert(&pool, &p2).await.unwrap();

    // Resolved papers under the default target are re-filed but not re-resolved,
    // so the (unreachable) resolver is never called.
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".into(),
        "http://127.0.0.1:1".into(),
    )
    .unwrap();
    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(dir.path(), &library),
        resolver,
        None,
    );
    let summary = refresh::run(&ctx, RefreshTarget::NeedsReview)
        .await
        .unwrap();
    assert_eq!(summary.refiled, 2);

    // First paper keeps the plain base; the second is disambiguated with a suffix.
    let got1 = papers::get(&pool, &p1.id).await.unwrap().unwrap();
    let got2 = papers::get(&pool, &p2.id).await.unwrap().unwrap();
    assert_eq!(got1.cite_key.as_deref(), Some("he2016deep"));
    assert_eq!(got1.rel_path, "he2016deep.pdf");
    assert_eq!(got2.cite_key.as_deref(), Some("he2016deepa"));
    assert_eq!(got2.rel_path, "he2016deepa.pdf");
    assert!(library.join("he2016deep.pdf").exists());
    assert!(library.join("he2016deepa.pdf").exists());
}

#[tokio::test]
async fn refresh_skips_a_trashed_paper() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    std::fs::create_dir_all(&library).unwrap();
    let hash = "trashedhash";
    let old = library.join(format!("{hash}.pdf"));
    support::write_test_pdf(&old, &["Whatever"]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let mut p = seed_paper(
        "01890000-0000-7000-8000-0000000000e7",
        hash,
        &format!("{hash}.pdf"),
        PaperStatus::Resolved,
    );
    p.meta.title = Some("Deep Residual Learning for Image Recognition".into());
    p.meta.authors = Authors(vec!["Kaiming He".into()]);
    p.meta.year = Some(2016);
    papers::insert(&pool, &p).await.unwrap();
    trash::soft_delete(&pool, &p.id).await.unwrap();

    // Explicitly refreshing a trashed paper is a no-op (frozen); its file is not moved.
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".into(),
        "http://127.0.0.1:1".into(),
    )
    .unwrap();
    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(dir.path(), &library),
        resolver,
        None,
    );
    let summary = refresh::run(&ctx, RefreshTarget::One(p.id.clone()))
        .await
        .unwrap();
    assert_eq!(summary.processed, 0);
    assert!(old.exists());
    assert!(!library.join("he2016deep.pdf").exists());
}

#[tokio::test]
async fn refile_copy_failure_keeps_db_and_file_consistent() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    let hash = "copyfailhash";
    let unsorted = library.join(format!("_unsorted/{hash}.pdf"));
    std::fs::create_dir_all(unsorted.parent().unwrap()).unwrap();
    let doi = "10.1145/3292500.3330701";
    support::write_test_pdf(&unsorted, &["Header", &format!("https://doi.org/{doi}")]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let p = seed_paper(
        "01890000-0000-7000-8000-0000000000c9",
        hash,
        &format!("_unsorted/{hash}.pdf"),
        PaperStatus::NeedsReview,
    );
    papers::insert(&pool, &p).await.unwrap();

    // Make the re-file destination impossible: a DIRECTORY occupies the
    // target path "wang2019kgat.pdf", so the copy must fail.
    std::fs::create_dir_all(library.join("wang2019kgat.pdf")).unwrap();

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;
    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(dir.path(), &library),
        Resolver::with_bases(None, server.uri(), server.uri()).unwrap(),
        None,
    );

    let summary = refresh::run(&ctx, RefreshTarget::NeedsReview)
        .await
        .unwrap();
    assert_eq!(summary.reresolved, 1);
    assert_eq!(summary.refiled, 0); // copy failed → not refiled

    // DB still points at the ORIGINAL path, and that file still exists:
    // metadata updated, location untouched, nothing orphaned.
    let got = papers::get(&pool, &p.id).await.unwrap().unwrap();
    assert_eq!(got.meta.status, PaperStatus::Resolved);
    assert_eq!(got.rel_path, format!("_unsorted/{hash}.pdf"));
    assert!(unsorted.exists());
}

#[tokio::test]
async fn refile_rolls_back_copy_when_update_fails() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    let hash = "rollbackhash";
    let unsorted = library.join(format!("_unsorted/{hash}.pdf"));
    std::fs::create_dir_all(unsorted.parent().unwrap()).unwrap();
    let doi = "10.1145/3292500.3330701";
    support::write_test_pdf(&unsorted, &["Header", &format!("https://doi.org/{doi}")]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();

    // Paper A: needs_review, will re-resolve to the KGAT record (and its DOI).
    let a = seed_paper(
        "01890000-0000-7000-8000-0000000000d1",
        hash,
        &format!("_unsorted/{hash}.pdf"),
        PaperStatus::NeedsReview,
    );
    papers::insert(&pool, &a).await.unwrap();

    // Paper B already owns that DOI (no cite_key, no file on disk → refresh
    // skips it), so A's post-copy papers::update hits the doi UNIQUE constraint.
    let mut b = seed_paper(
        "01890000-0000-7000-8000-0000000000d2",
        "otherhash",
        "missing/nowhere.pdf",
        PaperStatus::Resolved,
    );
    b.meta.doi = Some(doi.to_string());
    papers::insert(&pool, &b).await.unwrap();

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;
    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(dir.path(), &library),
        Resolver::with_bases(None, server.uri(), server.uri()).unwrap(),
        None,
    );

    // The per-paper error is logged, not propagated: run() succeeds.
    let summary = refresh::run(&ctx, RefreshTarget::NeedsReview)
        .await
        .unwrap();
    assert_eq!(summary.refiled, 0);

    // Rollback: the copied file at the would-be new path was removed…
    assert!(!library.join("wang2019kgat.pdf").exists());
    // …and A's row is untouched (old path, still needs_review, no DOI).
    let got = papers::get(&pool, &a.id).await.unwrap().unwrap();
    assert_eq!(got.rel_path, format!("_unsorted/{hash}.pdf"));
    assert_eq!(got.meta.status, PaperStatus::NeedsReview);
    assert_eq!(got.meta.doi, None);
    assert!(unsorted.exists());
}
