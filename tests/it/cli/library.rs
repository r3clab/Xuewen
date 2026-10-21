use super::{fails, library, ok};

#[tokio::test]
async fn list_show_and_stats_read_the_library() {
    let (pool, config) = library().await;
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
    let (pool, config) = library().await;
    assert!(ok(&config, &["citations", "p1"]).starts_with("no parsed references yet"));

    sqlx::query("INSERT INTO citation_parses (paper_id, refs, parsed, model, created_at) VALUES ('p1', ?, ?, 'heuristic-v1', '2026-07-08T00:00:00Z')")
        .bind(r#"["[1] A. Smith and B. Jones. Deep Things. NeurIPS, 2020.", "[2] garbled"]"#)
        .bind(r#"[{"src":"h1","authors":["A. Smith","B. Jones"],"title":"Deep Things","venue":"NeurIPS","year":2020,"doi":"10.1/x"}, null]"#)
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
    let (_pool, config) = library().await;
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
    let (_pool, config) = library().await;
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
    assert!(fails(&config, &["project", "new", "kernels"])
        .contains("a project with that name already exists"));
    assert!(fails(&config, &["project", "rename", "kernels", "  "])
        .contains("project name cannot be empty"));
}

#[tokio::test]
async fn annotation_add_validates_and_lists_the_note() {
    let (_pool, config) = library().await;
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
    assert!(fails(
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
    .contains("invalid value 'ink' for '--color <COLOR>'"));
    assert!(fails(
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
    .contains("note must be at most 10000 characters"));
}
