use std::path::{Path, PathBuf};

use xuewen::search::fts::{FtsIndex, PaperDoc};
use xuewen::search::vector::{OpenMode, VectorStore};

use super::{library_with, ok, stderr, xuewen};
use crate::support;

/// One paper `p1` whose PDF says "zyzzyva", nothing indexed yet.
async fn unindexed_library(extra: &str) -> (sqlx::SqlitePool, PathBuf) {
    let (pool, config) = library_with(extra).await;
    support::write_test_pdf(
        &dir(&config).join("library/missing.pdf"),
        &["zyzzyva marks the spot"],
    );
    (pool, config)
}

fn dir(config: &Path) -> &Path {
    config.parent().unwrap()
}

async fn index_rows(pool: &sqlx::SqlitePool) -> Vec<(String, Option<String>, Option<String>, i64)> {
    sqlx::query_as(
        "SELECT paper_id, fts_indexed_at, vectors_indexed_at, fts_attempts FROM search_index",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

fn hold_fts_writer(config: &Path) -> FtsIndex {
    let (idx, _) = FtsIndex::open(&dir(config).join("idx")).unwrap();
    idx.upsert(&PaperDoc {
        id: "holder".into(),
        ..Default::default()
    })
    .unwrap();
    idx
}

#[tokio::test]
async fn search_catches_up_a_never_indexed_library() {
    let (_pool, config) = unindexed_library("").await;
    let found = ok(&config, &["search", "--keyword-only", "zyzzyva"]);
    assert!(found.contains("Paper p1"), "got: {found}");
}

#[tokio::test]
async fn trash_and_restore_update_the_index_before_the_command_returns() {
    let (pool, config) = unindexed_library("").await;
    ok(&config, &["search", "--keyword-only", "zyzzyva"]);
    assert_eq!(index_rows(&pool).await.len(), 1);

    let out = xuewen(&config, &["delete", "p1", "--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stderr(&out), "");
    assert!(index_rows(&pool).await.is_empty(), "tombstone swept");

    ok(&config, &["restore", "p1"]);
    let rows = index_rows(&pool).await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].1.is_some(), "re-indexed by the restore itself");
}

#[tokio::test]
async fn an_added_note_is_searchable_and_only_the_fts_tier_runs() {
    let (pool, config) = unindexed_library("").await;
    ok(
        &config,
        &["annotation", "add", "p1", "--page", "1", "quokka insight"],
    );
    let rows = index_rows(&pool).await;
    assert!(rows[0].1.is_some() && rows[0].2.is_none());

    let found = ok(&config, &["search", "--keyword-only", "in:notes quokka"]);
    assert!(found.contains("Paper p1"), "got: {found}");
}

#[tokio::test]
async fn a_held_index_writer_is_skipped_without_recording_failures() {
    let (pool, config) = unindexed_library("").await;
    let _holder = hold_fts_writer(&config);

    let out = xuewen(
        &config,
        &["annotation", "add", "p1", "--page", "1", "quokka insight"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stderr(&out),
        "note: search index not updated — the full-text index is held by another xuewen process; it catches up on its next sweep\n"
    );
    assert!(index_rows(&pool).await.is_empty());
}

#[tokio::test]
async fn a_held_vector_store_is_skipped() {
    let (pool, config) = unindexed_library(
        "[ai]\nbase_url = \"http://127.0.0.1:1/v1\"\napi_key = \"k\"\n\n[ai.embedding]\nmodel = \"m1\"\ndims = 4\n",
    )
    .await;
    let _holder = VectorStore::open(&dir(&config).join("vectors"), 4, OpenMode::LoadOrCreate)
        .await
        .unwrap();

    let out = xuewen(&config, &["delete", "p1", "--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("the vector store is held by another xuewen process"),
        "got: {}",
        stderr(&out)
    );
    assert!(index_rows(&pool).await.is_empty());
}

#[tokio::test]
async fn read_only_commands_never_touch_the_index() {
    let (_pool, config) = unindexed_library("").await;
    let _holder = hold_fts_writer(&config);

    for args in [
        &["tag", "add", "p1", "nlp"][..],
        &["star", "p1"],
        &["annotation", "list", "p1"],
    ] {
        let out = xuewen(&config, args);
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        assert_eq!(stderr(&out), "", "{args:?}");
    }
}
