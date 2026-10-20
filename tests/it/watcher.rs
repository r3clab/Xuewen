use crate::support;

use std::time::Duration;

use tokio_util::sync::CancellationToken;
use xuewen::db;
use xuewen::ingest::pipeline::{IngestCtx, Libraries};

#[tokio::test]
async fn watcher_catches_up_and_watches_new_files() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();

    // One file present BEFORE the watcher starts (exercises catch-up)...
    support::write_test_pdf(&inbox.join("existing.pdf"), &["Existing Paper Title Here"]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());

    // Move owned values into the watcher task (so it satisfies 'static).
    let inbox_for_task = inbox.clone();
    let db_url = url.clone();
    let cancel = CancellationToken::new();
    let watch_cancel = cancel.clone();
    let handle = tokio::spawn(async move {
        let pool = db::connect(&db_url).await.unwrap();
        let ctx = IngestCtx::new(
            pool,
            Libraries::under(
                &inbox_for_task,
                &inbox_for_task.parent().unwrap().join("library"),
            ),
            support::offline_resolver(),
            None,
        );
        xuewen::ingest::watcher::run(&ctx, &inbox_for_task, watch_cancel).await
    });

    // The watch is registered before catch-up, so once the existing file is
    // processed the live path is armed; then drop a second file (exercises the
    // live notify path).
    let processed = inbox.join("_processed");
    let existing_ok = support::eventually(Duration::from_secs(10), || {
        processed.join("existing.pdf").exists()
    })
    .await;
    assert!(existing_ok, "catch-up file was not ingested");
    support::write_test_pdf(&inbox.join("dropped.pdf"), &["Freshly Dropped Paper Title"]);
    let dropped_ok = support::eventually(Duration::from_secs(10), || {
        processed.join("dropped.pdf").exists()
    })
    .await;

    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("watcher stops once cancelled")
        .unwrap()
        .unwrap();

    assert!(dropped_ok, "live-dropped file was not ingested");

    // Both are recorded.
    let pool = db::connect(&url).await.unwrap();
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM papers")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 2);
}
