use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use tokio_util::sync::CancellationToken;

use crate::ingest::pipeline::IngestCtx;
use crate::library::naming::files::move_to_async;

/// Tunables for the watch loop. `Default` is for production; tests use small values.
#[derive(Debug, Clone)]
struct WatchConfig {
    /// How long to wait between file-size polls when checking for stability.
    poll_interval: Duration,
    /// Number of consecutive unchanged polls required to consider a file stable.
    stable_polls: u32,
    /// Maximum number of polls before giving up waiting for stability.
    max_polls: u32,
    /// Maximum ingest attempts before quarantining.
    max_attempts: u32,
    /// Base delay for exponential backoff between ingest attempts.
    retry_base_delay: Duration,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(400),
            stable_polls: 2,
            max_polls: 25,
            max_attempts: 3,
            retry_base_delay: Duration::from_secs(1),
        }
    }
}

fn is_pdf(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
}

/// List the top-level `*.pdf` files currently in the inbox (non-recursive, so the
/// `_processed`/`_failed` subdirectories are skipped). Sorted for determinism.
async fn catch_up_scan(inbox: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut entries = match tokio::fs::read_dir(inbox).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if is_pdf(&path) && tokio::fs::metadata(&path).await.is_ok_and(|m| m.is_file()) {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// Wait until the file's size is stable (unchanged for `stable_polls` consecutive
/// polls). Returns `false` if the file vanished; on timeout returns `false` (the
/// file is still changing, so it is left for a later event/restart). Guards
/// against ingesting a half-written download.
async fn stabilize(path: &Path, cfg: &WatchConfig) -> bool {
    let mut last: Option<u64> = None;
    let mut steady = 0u32;
    for _ in 0..cfg.max_polls {
        let size = match tokio::fs::metadata(path).await {
            Ok(m) => m.len(),
            Err(_) => return false, // gone
        };
        if size > 0 && Some(size) == last {
            steady += 1;
            if steady >= cfg.stable_polls {
                return true;
            }
        } else {
            steady = 0;
            last = Some(size);
        }
        tokio::time::sleep(cfg.poll_interval).await;
    }
    // Never stabilized within the window (still being written). Skip this round;
    // a later filesystem event or the next startup catch-up will retry it.
    false
}

/// Ingest a file, retrying with exponential backoff. Re-ingestion is safe: the
/// content-hash/identifier dedup makes a retry after a partial failure report
/// the existing row (`Duplicate`/`SameWork`/`InTrash`).
async fn ingest_with_retry(
    ctx: &IngestCtx,
    cfg: &WatchConfig,
    path: &Path,
) -> Result<crate::ingest::pipeline::Outcome> {
    let mut delay = cfg.retry_base_delay;
    let mut last_err = None;
    for attempt in 1..=cfg.max_attempts {
        match ctx.ingest_file(path).await {
            Ok(o) => return Ok(o),
            Err(e) => {
                tracing::warn!("ingest attempt {attempt}/{} failed: {e}", cfg.max_attempts);
                // pdftotext exiting non-zero is deterministic for the same
                // (stabilized) bytes: retrying cannot succeed, so skip the
                // backoff — which would block the sequential inbox queue —
                // and let the caller quarantine now.
                let permanent = e
                    .chain()
                    .any(|c| c.downcast_ref::<crate::pdf::PdftotextFailed>().is_some());
                last_err = Some(e);
                if permanent {
                    break;
                }
                if attempt < cfg.max_attempts {
                    tokio::time::sleep(delay).await;
                    delay *= 2;
                }
            }
        }
    }
    Err(last_err.expect("at least one attempt ran"))
}

/// Stabilize then ingest one path; on repeated failure, quarantine to
/// `failed_dir`. Returns whether an ingest ran: even a failed one can have
/// stored the paper before failing to archive the file.
async fn process_one(ctx: &IngestCtx, failed_dir: &Path, cfg: &WatchConfig, path: &Path) -> bool {
    if !stabilize(path, cfg).await {
        return false; // file vanished (e.g. already processed by a prior event)
    }
    match ingest_with_retry(ctx, cfg, path).await {
        Ok(outcome) => tracing::info!("processed {}: {outcome:?}", path.display()),
        Err(e) => {
            tracing::error!(
                "giving up on {}: {e}; quarantining to _failed",
                path.display()
            );
            if let Err(mv) = move_to_async(path, failed_dir).await {
                tracing::error!("could not quarantine {}: {mv}", path.display());
            }
        }
    }
    true
}

/// Run the watch loop: catch up on existing files, then watch for new ones until
/// `cancel` fires, calling `library_changed` after every ingest. Blocks (the
/// `notify` watcher is held for the loop's life).
pub async fn run(
    ctx: &IngestCtx,
    inbox: &Path,
    cancel: CancellationToken,
    library_changed: &(dyn Fn() + Sync),
) -> Result<()> {
    let cfg = WatchConfig::default();
    let failed_dir = ctx.dirs.failed_dir.clone();

    // Live watch. `notify` calls the handler on its own thread; forward pdf paths
    // over an unbounded channel (send() is non-blocking and runtime-agnostic).
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PathBuf>();
    let mut watcher =
        notify::recommended_watcher(move |res: notify::Result<notify::Event>| match res {
            Ok(event) => {
                if matches!(
                    event.kind,
                    notify::EventKind::Create(_) | notify::EventKind::Modify(_)
                ) {
                    for p in event.paths {
                        if is_pdf(&p) {
                            let _ = tx.send(p);
                        }
                    }
                }
            }
            Err(e) => tracing::warn!("watch event error: {e}"),
        })?;
    // Registered before the catch-up scan so a file landing in between is not
    // missed; one seen by both paths is harmless (`stabilize` skips it once moved).
    watcher.watch(inbox, RecursiveMode::NonRecursive)?;
    tracing::info!("watching {}", inbox.display());

    // Catch-up: ingest anything already sitting in the inbox.
    for path in catch_up_scan(inbox).await? {
        if cancel.is_cancelled() {
            return Ok(());
        }
        if process_one(ctx, &failed_dir, &cfg, &path).await {
            library_changed();
        }
    }

    loop {
        let path = tokio::select! {
            biased;
            () = cancel.cancelled() => break,
            path = rx.recv() => match path {
                Some(path) => path,
                None => break,
            },
        };
        if process_one(ctx, &failed_dir, &cfg, &path).await {
            library_changed();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::pipeline::Libraries;
    use crate::metadata::resolve::Resolver;
    use crate::testutil::write_pdf;
    use sqlx::SqlitePool;
    use std::fs::File;
    use std::io::Write;

    fn fast_cfg() -> WatchConfig {
        WatchConfig {
            poll_interval: Duration::from_millis(5),
            stable_polls: 1,
            max_polls: 5,
            max_attempts: 2,
            retry_base_delay: Duration::from_millis(1),
        }
    }

    async fn temp_pool(dir: &Path) -> SqlitePool {
        let url = format!("sqlite:{}", dir.join("library.db").display());
        crate::db::connect(&url).await.unwrap()
    }

    #[tokio::test]
    async fn catch_up_scan_lists_top_level_pdfs_only() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path();
        File::create(inbox.join("b.pdf")).unwrap();
        File::create(inbox.join("a.pdf")).unwrap();
        File::create(inbox.join("notes.txt")).unwrap();
        std::fs::create_dir_all(inbox.join("_processed")).unwrap();
        File::create(inbox.join("_processed/old.pdf")).unwrap();

        assert!(catch_up_scan(&inbox.join("missing"))
            .await
            .unwrap()
            .is_empty());
        let found = catch_up_scan(inbox).await.unwrap();
        assert_eq!(
            found,
            vec![inbox.join("a.pdf"), inbox.join("b.pdf")] // sorted, top-level only
        );
    }

    #[tokio::test]
    async fn stabilize_true_for_steady_file_false_for_missing() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.pdf");
        let mut fh = File::create(&f).unwrap();
        fh.write_all(b"some bytes").unwrap();
        fh.sync_all().unwrap();
        assert!(stabilize(&f, &fast_cfg()).await);
        assert!(!stabilize(&dir.path().join("missing.pdf"), &fast_cfg()).await);
    }

    #[tokio::test]
    async fn process_one_ingests_and_moves_to_processed() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        let pdf = inbox.join("paper.pdf");
        write_pdf(&pdf, &["A Paper With No Identifier Here"]);

        let pool = temp_pool(dir.path()).await;
        let ctx = IngestCtx::new(
            pool.clone(),
            Libraries::under(&inbox, &dir.path().join("library")),
            Resolver::offline(),
            None,
        );

        process_one(&ctx, &inbox.join("_failed"), &fast_cfg(), &pdf).await;

        assert!(inbox.join("_processed/paper.pdf").exists());
        assert!(!pdf.exists());
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM papers")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn process_one_quarantines_unreadable_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        let bad = inbox.join("bad.pdf");
        std::fs::write(&bad, b"this is not a pdf").unwrap();

        let pool = temp_pool(dir.path()).await;
        let ctx = IngestCtx::new(
            pool.clone(),
            Libraries::under(&inbox, &dir.path().join("library")),
            Resolver::offline(),
            None,
        );
        let failed = inbox.join("_failed");

        // A backoff so large the test would blow past its bound if the
        // permanent-failure fast path (pdftotext exited non-zero → no retry)
        // regressed into sleeping between attempts.
        let cfg = WatchConfig {
            retry_base_delay: Duration::from_secs(60),
            ..fast_cfg()
        };
        let started = std::time::Instant::now();
        process_one(&ctx, &failed, &cfg, &bad).await;
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "an unreadable PDF must quarantine without retry backoff"
        );

        assert!(failed.join("bad.pdf").exists(), "should be quarantined");
        assert!(!bad.exists());
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM papers")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }
}
