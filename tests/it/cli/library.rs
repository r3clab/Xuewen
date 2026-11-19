use serde_json::json;
use sqlx::SqlitePool;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{fails, library, library_with, ok};

#[tokio::test]
async fn list_show_and_stats_read_the_library() {
    let (pool, config, _dir) = library().await;
    let mut p2 = xuewen::testutil::paper(
        "p2",
        "Second Paper",
        xuewen::library::model::PaperStatus::NeedsReview,
    );
    p2.meta.year = Some(2024);
    xuewen::library::papers::insert(&pool, &p2).await.unwrap();
    ok(&config, &["star", "p2"]);

    assert_eq!(
        ok(&config, &["list", "--sort", "year_desc"]),
        "p2  2024  *p2  Second Paper\np1  2020   p1  Paper p1\n"
    );
    assert_eq!(
        ok(&config, &["list", "--starred"]),
        "p2  2024  *p2  Second Paper\n"
    );
    assert_eq!(
        ok(&config, &["list", "--query", "nothing-like-it"]),
        "no papers\n"
    );
    assert!(fails(&config, &["list", "--sort", "random"]).contains("possible values"));

    assert_eq!(
        ok(&config, &["show", "p1"]),
        "Paper p1\n  id:       p1\n  cite key: p1\n  authors:  Ada Lovelace\n  venue:    KDD 2020\n  \
         status:   resolved\n  added:    2026-07-07T00:00:00Z\n\nAbstract\n  An abstract.\n"
    );
    assert_eq!(
        ok(&config, &["stats"]),
        "2 papers (1 resolved, 1 need review)\n"
    );
}

#[tokio::test]
async fn citations_print_the_cached_parse() {
    let (pool, config, _dir) = library().await;
    assert!(ok(&config, &["citations", "p1"]).starts_with("no parsed references yet"));

    sqlx::query("INSERT INTO citation_parses (paper_id, refs, parsed, model, created_at) VALUES ('p1', ?, ?, 'heuristic-v1', '2026-07-08T00:00:00Z')")
        .bind(r#"["[1] A. Smith and B. Jones. Deep Things. NeurIPS, 2020.", "[2] garbled"]"#)
        .bind(r#"[{"src":{"heuristic":1},"authors":["A. Smith","B. Jones"],"title":"Deep Things","venue":"NeurIPS","year":2020,"doi":"10.1/x"}, null]"#)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        ok(&config, &["citations", "p1"]),
        "2 references (1 parsed; heuristic-v1, 2026-07-08T00:00:00Z)\n  \
         1. A. Smith, B. Jones (2020). Deep Things. NeurIPS. doi:10.1/x\n  \
         2. (unparsed) [2] garbled\n"
    );
}

#[tokio::test]
async fn name_prints_sets_and_clears() {
    let (_pool, config, _dir) = library().await;
    assert_eq!(ok(&config, &["name", "p1"]), "p1 has no name\n");
    assert_eq!(ok(&config, &["name", "p1", "  AFL  "]), "named p1 AFL\n");
    assert_eq!(ok(&config, &["name", "p1"]), "AFL\n");
    assert!(ok(&config, &["show", "p1"]).contains("  name:     AFL\n"));
    assert!(fails(&config, &["name", "p1", &"x".repeat(500)]).contains("name is too long"));
    assert_eq!(
        ok(&config, &["name", "p1", "--clear"]),
        "cleared the name of p1\n"
    );
    assert_eq!(ok(&config, &["name", "p1"]), "p1 has no name\n");
}

#[tokio::test]
async fn project_rename_keeps_names_unique() {
    let (_pool, config, _dir) = library().await;
    ok(&config, &["project", "new", "fuzzing"]);
    ok(&config, &["project", "new", "kernels"]);
    assert_eq!(
        ok(&config, &["project", "rename", "fuzzing", "Fuzzing 2"]),
        "renamed project fuzzing to Fuzzing 2\n"
    );
    assert!(
        fails(&config, &["project", "rename", "Fuzzing 2", "kernels"])
            .contains("a project with that name already exists")
    );
    assert!(
        fails(&config, &["project", "new", "kernels"])
            .contains("a project with that name already exists")
    );
    assert!(
        fails(&config, &["project", "rename", "kernels", "  "])
            .contains("project name cannot be empty")
    );
}

#[tokio::test]
async fn tag_rename_reports_a_taken_name() {
    let (_pool, config, _dir) = library().await;
    ok(&config, &["tag", "add", "p1", "ml"]);
    ok(&config, &["tag", "add", "p1", "nlp"]);
    assert_eq!(
        fails(&config, &["tag", "rename", "nlp", "ML"]),
        "Error: a tag with that name already exists\n"
    );
}

#[tokio::test]
async fn export_takes_any_filter_as_its_selector() {
    let (_pool, config, _dir) = library().await;
    for [flag, value] in [["--status", "resolved"], ["--query", "Paper"]] {
        let out = ok(&config, &["export", flag, value]);
        assert!(out.starts_with('@'), "{flag}: {out}");
    }
    assert!(fails(&config, &["export"]).contains("specify a paper id, --all, or a filter"));
    assert!(
        fails(&config, &["export", "p1", "--status", "resolved"]).contains("cannot be used with")
    );
    assert!(fails(&config, &["export", "--all", "--format", "ris"]).contains("possible values"));
}

#[tokio::test]
async fn annotation_add_validates_and_lists_the_note() {
    let (_pool, config, _dir) = library().await;
    let added = ok(
        &config,
        &[
            "annotation",
            "add",
            "p1",
            "--page",
            "3",
            "--quote",
            "the key lemma",
            "check the proof",
        ],
    );
    assert!(
        added.starts_with("added ") && added.ends_with(" on p3\n"),
        "got: {added}"
    );
    let listed = ok(&config, &["annotation", "list", "p1"]);
    assert!(
        listed.contains("check the proof") && listed.contains("the key lemma"),
        "got: {listed}"
    );

    assert!(
        fails(&config, &["annotation", "add", "p1", "--page", "0", "x"])
            .contains("invalid value '0' for '--page <PAGE>'")
    );
    assert!(
        fails(
            &config,
            &[
                "annotation",
                "add",
                "p1",
                "--page",
                "1",
                "--color",
                "ink",
                "x"
            ]
        )
        .contains("invalid value 'ink' for '--color <COLOR>'")
    );
    assert!(
        fails(
            &config,
            &[
                "annotation",
                "add",
                "p1",
                "--page",
                "1",
                &"n".repeat(10_001)
            ]
        )
        .contains("note must be at most 10000 characters")
    );
}

async fn summary_model(pool: &SqlitePool) -> String {
    sqlx::query_scalar("SELECT model FROM paper_summaries WHERE paper_id = 'p1'")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn summarize_replaces_a_summary_only_with_a_new_one() {
    let upstream = MockServer::start().await;
    let (pool, config, _dir) = library_with(&format!(
        "[ai]\nbase_url = {:?}\napi_key = \"k\"\n\n[ai.summary]\nmodel = \"mock-1\"\n",
        upstream.uri()
    ))
    .await;
    let summary = json!({
        "tldr": "One line.",
        "problem": "Gap.",
        "approach": "Idea.",
        "results": "Better.",
        "limitations": "Small."
    });
    sqlx::query("INSERT INTO paper_summaries (paper_id, summary, model, generated_at) VALUES ('p1', ?, 'old-1', '2026-07-08T00:00:00Z')")
        .bind(summary.to_string())
        .execute(&pool)
        .await
        .unwrap();

    let outage = Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(400))
        .mount_as_scoped(&upstream)
        .await;
    assert_eq!(
        ok(&config, &["summarize", "p1"]),
        "no summary generated (see logs)\n"
    );
    assert_eq!(
        ok(&config, &["summarize", "--all"]),
        "generated 0 summaries\n"
    );
    assert_eq!(summary_model(&pool).await, "old-1");
    drop(outage);

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"role": "assistant", "content": summary.to_string()}}]
        })))
        .mount(&upstream)
        .await;
    assert_eq!(
        ok(&config, &["summarize", "--all"]),
        "generated 1 summaries\n"
    );
    assert_eq!(summary_model(&pool).await, "mock-1");
}
