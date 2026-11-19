use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::db::ChangeWatch;
use crate::library::model::Paper;
use crate::pdf::Pages;
use crate::search::planner::VectorWork;
use crate::search::store::{ErrorTier, VectorsDone};
use crate::search::vector::{OpenError, OpenMode};
use crate::search::{chunker, fts, planner, store, vector, SearchService, Semantic};
use crate::tasks::Tasks;

/// Docs staged per Tantivy commit: few enough segments for a rebuild, yet a
/// long sweep's progress still shows up in searches as it goes.
const FTS_BATCH: usize = 64;
/// How often a loop whose locks another process holds tries them again.
const CLAIM_RETRY: Duration = Duration::from_secs(2);
/// The loop sweeps on every library change and backoff deadline; this only
/// bounds how long a missed signal could go unnoticed.
const IDLE_SWEEP: Duration = Duration::from_secs(600);

#[derive(Debug, Default)]
pub struct SweepSummary {
    pub indexed: usize,
    pub(crate) deindexed: usize,
    pub failed: usize,
    pub(crate) vectors_touched: usize,
    /// When the earliest backed-off work falls due.
    pub(crate) retry_at: Option<DateTime<Utc>>,
}

impl SweepSummary {
    pub(crate) fn is_empty(&self) -> bool {
        self.indexed + self.deindexed + self.failed == 0
    }
}

/// Which tiers `rebuild` drops and re-derives.
#[derive(Debug, Clone, Copy)]
pub(crate) enum RebuildScope {
    Fts,
    Vectors,
    Both,
}

/// Which tiers a sweep may write. `FtsOnly` plans no vector work — so it
/// never calls the embedding API — yet still deletes a tombstone's points,
/// because it keeps the real vector store rather than pretending there is
/// none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    Full,
    FtsOnly,
}

/// Why a catch-up left the index to somebody else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Skipped {
    VectorStoreHeld,
    IndexWriterHeld,
}

impl std::fmt::Display for Skipped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::VectorStoreHeld => "the vector store is held by another xuewen process",
            Self::IndexWriterHeld => "the full-text index is held by another xuewen process",
        })
    }
}

#[derive(Debug)]
pub(crate) enum CatchUp {
    Swept(SweepSummary),
    Skipped(Skipped),
}

/// Drop and re-derive the search indexes from SQLite + PDFs (`xuewen index
/// rebuild`). Nothing is wiped until both indexes are known to be ours: the
/// Tantivy writer lock is taken first, and the vector store's lock (and, for
/// a vectors rebuild, the store recreated under it) before the Tantivy
/// directory goes. `SearchService::open` then detects the fresh Tantivy
/// directory and clears the FTS stamps itself, a recreated store clears the
/// vector stamps, and one full sweep follows.
pub(crate) async fn rebuild(
    pool: sqlx::SqlitePool,
    search: &crate::config::SearchConfig,
    ai: &crate::config::AiConfig,
    library_root: &Path,
    scope: RebuildScope,
) -> Result<SweepSummary> {
    let (do_fts, do_vectors) = match scope {
        RebuildScope::Fts => (true, false),
        RebuildScope::Vectors => (false, true),
        RebuildScope::Both => (true, true),
    };
    let fts_lock = if do_fts {
        Some(fts::DirWriterLock::acquire(&search.index_dir)?)
    } else {
        None
    };
    let mode = if do_vectors {
        OpenMode::Recreate
    } else {
        OpenMode::LoadOrCreate
    };
    let semantic = Semantic::open(&pool, search, ai, mode).await?;
    match &semantic {
        Semantic::Unavailable(e @ OpenError::Locked { .. }) => {
            anyhow::bail!("{e} — stop it and retry")
        }
        Semantic::Unavailable(e) if do_vectors => anyhow::bail!("{e}"),
        _ => {}
    }
    if let Some(lock) = fts_lock {
        if let Err(e) = lock.wipe() {
            if let Some(store) = semantic.store() {
                store.close_gracefully().await;
            }
            return Err(e.context(format!(
                "could not clear search index dir {}",
                search.index_dir.display()
            )));
        }
    }
    let svc = SearchService::open(pool.clone(), search, semantic).await?;
    if svc.claim_writer().await? == fts::WriterClaim::Busy {
        svc.close_vectors().await;
        anyhow::bail!("{} — stop it and retry", Skipped::IndexWriterHeld);
    }
    store::clear_stamps(&pool, do_fts, do_vectors).await?;
    let summary = sweep(&svc, library_root).await;
    svc.close_vectors().await;
    summary
}

/// Bring the index up to date unless another process owns it. Nothing is
/// planned until both locks are known to be ours, so a held index is skipped
/// cleanly instead of failing every stale paper into the shared backoff.
pub(crate) async fn catch_up(
    svc: &SearchService,
    library_root: &Path,
    scope: Scope,
) -> Result<CatchUp> {
    if matches!(
        svc.semantic,
        Semantic::Unavailable(OpenError::Locked { .. })
    ) {
        return Ok(CatchUp::Skipped(Skipped::VectorStoreHeld));
    }
    if svc.claim_writer().await? == fts::WriterClaim::Busy {
        return Ok(CatchUp::Skipped(Skipped::IndexWriterHeld));
    }
    Ok(CatchUp::Swept(
        sweep_scoped(svc, library_root, scope).await?,
    ))
}

/// `catch_up` for a process that holds no search service of its own: open
/// both indexes, sweep, and let go of the vector store again.
pub(crate) async fn catch_up_fresh(
    pool: sqlx::SqlitePool,
    search: &crate::config::SearchConfig,
    ai: &crate::config::AiConfig,
    library_root: &Path,
    scope: Scope,
) -> Result<CatchUp> {
    let semantic = Semantic::open(&pool, search, ai, OpenMode::LoadOrCreate).await?;
    let svc = SearchService::open(pool, search, semantic).await?;
    let caught_up = catch_up(&svc, library_root, scope).await;
    svc.close_vectors().await;
    caught_up
}

/// One full pass: remove tombstones, (re)index every stale paper.
/// Individual paper failures are recorded (with backoff) and never abort
/// the sweep.
pub async fn sweep(svc: &SearchService, library_root: &Path) -> Result<SweepSummary> {
    sweep_scoped(svc, library_root, Scope::Full).await
}

pub(crate) async fn sweep_scoped(
    svc: &SearchService,
    library_root: &Path,
    scope: Scope,
) -> Result<SweepSummary> {
    let papers = svc.paper_states().await?;
    let rows = store::all_index_rows(&svc.pool).await?;
    let rows_by_id: HashMap<&str, &store::IndexRow> =
        rows.iter().map(|r| (r.paper_id.as_str(), r)).collect();
    let tier = svc.semantic.tier();
    let plan = planner::plan(
        &papers,
        &rows,
        tier.filter(|_| scope == Scope::Full)
            .map(|t| t.embedder.model()),
        Utc::now(),
    );
    let mut sweep = Sweep {
        svc,
        summary: SweepSummary {
            retry_at: plan.retry_at,
            ..SweepSummary::default()
        },
        staged: Vec::new(),
    };
    let tombstones: Vec<&store::IndexRow> = plan
        .deindex
        .iter()
        .filter_map(|id| rows_by_id.get(id.as_str()).copied())
        .collect();
    sweep.deindex(&tombstones).await;
    for work in &plan.index {
        sweep.index(library_root, work, scope).await;
        if sweep.staged.len() >= FTS_BATCH {
            sweep.commit_staged().await;
        }
    }
    sweep.commit_staged().await;
    let summary = sweep.summary;
    if let Some(tier) = tier.filter(|_| summary.vectors_touched > 0) {
        if let Err(e) = tier.store.optimize().await {
            tracing::warn!("vector store optimize: {e:#}");
        }
    }
    Ok(summary)
}

/// A Tantivy doc written but not yet committed.
struct Staged {
    paper_id: String,
    doc_hash: String,
    /// No other tier of this paper failed in this sweep.
    clean: bool,
}

struct Sweep<'a> {
    svc: &'a SearchService,
    summary: SweepSummary,
    staged: Vec<Staged>,
}

impl Sweep<'_> {
    /// Log a failure and charge it to `tier`'s backoff.
    async fn record(&self, paper_id: &str, tier: ErrorTier, e: &anyhow::Error) {
        tracing::warn!("index {paper_id}: {e:#}");
        if let Err(re) =
            store::record_error(&self.svc.pool, paper_id, &format!("{e:#}"), tier).await
        {
            tracing::warn!("index {paper_id}: could not record the failure for backoff: {re:#}");
        }
    }

    async fn fail(&mut self, paper_id: &str, tier: ErrorTier, e: anyhow::Error) {
        self.record(paper_id, tier, &e).await;
        self.summary.failed += 1;
    }

    async fn index(&mut self, library_root: &Path, work: &planner::Work, scope: Scope) {
        let svc = self.svc;
        let id = work.paper_id.as_str();
        // Failures before the tiers split (lookup, extraction, chunking)
        // charge BOTH: the broken precursor serves both tiers, so neither may
        // hot-loop on it. Without an embedder there is no vector tier to
        // hot-loop — and a `vec_last_error` written then would never be
        // cleared, since no vector pass ever runs to clear it — so the charge
        // narrows to FTS alone. An FTS-only pass narrows it too: it never ran
        // the vector tier.
        let pre_tier = if scope == Scope::Full && svc.semantic.tier().is_some() {
            ErrorTier::Both
        } else {
            ErrorTier::Fts
        };
        let paper = match crate::library::papers::get(&svc.pool, id).await {
            Ok(Some(paper)) => paper,
            Ok(None) => return, // purged since the plan was computed; tombstone next sweep
            Err(e) => return self.fail(id, pre_tier, e).await,
        };
        let body = match derive_body(svc, library_root, &paper, work.reextract).await {
            Ok(body) => body,
            Err(e) => return self.fail(id, pre_tier, e).await,
        };

        let doc_hash = if work.fts {
            match stage_doc(svc, &paper, &body.chunks).await {
                Ok(doc_hash) => Some(doc_hash),
                Err(e) => return self.fail(id, ErrorTier::Fts, e).await,
            }
        } else {
            None
        };
        // A re-extracted body that changed has cleared the vector stamp:
        // re-embed all of it now rather than the summary alone.
        let vectors = match work.vectors {
            VectorWork::Summary if body.changed => VectorWork::All,
            planned => planned,
        };
        let clean = match vectors {
            VectorWork::None => true,
            vectors => match index_vectors(svc, &paper, &body.chunks, vectors).await {
                Ok(()) => {
                    self.summary.vectors_touched += 1;
                    true
                }
                Err(e) => {
                    self.fail(id, ErrorTier::Vectors, e).await;
                    false
                }
            },
        };
        match doc_hash {
            Some(doc_hash) => self.staged.push(Staged {
                paper_id: paper.id,
                doc_hash,
                clean,
            }),
            None if clean => self.summary.indexed += 1,
            None => {}
        }
    }

    /// Commit the staged docs and stamp them. A failed commit rolls every one
    /// of them back, so each is charged to the FTS backoff.
    async fn commit_staged(&mut self) {
        if self.staged.is_empty() {
            return;
        }
        let staged = std::mem::take(&mut self.staged);
        let stamped = match self.svc.fts_commit().await {
            Ok(()) => {
                let docs: Vec<(&str, &str)> = staged
                    .iter()
                    .map(|s| (s.paper_id.as_str(), s.doc_hash.as_str()))
                    .collect();
                store::mark_fts_done(&self.svc.pool, &docs)
                    .await
                    .context("stamping the committed docs")
            }
            Err(e) => Err(e.context("committing the full-text index")),
        };
        for s in &staged {
            match &stamped {
                Ok(()) if s.clean => self.summary.indexed += 1,
                Ok(()) => {}
                Err(e) => {
                    self.record(&s.paper_id, ErrorTier::Fts, e).await;
                    if s.clean {
                        self.summary.failed += 1;
                    }
                }
            }
        }
    }

    async fn deindex(&mut self, tombstones: &[&store::IndexRow]) {
        let docs: Vec<String> = tombstones
            .iter()
            .filter(|r| r.fts_indexed_at.is_some())
            .map(|r| r.paper_id.clone())
            .collect();
        let fts_gone = if docs.is_empty() {
            true
        } else {
            let removed = match self.svc.fts_stage_deletes(docs).await {
                Ok(()) => self.svc.fts_commit().await,
                Err(e) => Err(e),
            };
            match removed {
                Ok(()) => true,
                Err(e) => {
                    tracing::warn!("deindex: removing full-text docs: {e:#}");
                    false
                }
            }
        };
        for row in tombstones {
            if !fts_gone && row.fts_indexed_at.is_some() {
                self.summary.failed += 1; // the row stays for the next sweep
                continue;
            }
            match deindex_paper(self.svc, row).await {
                Ok(false) => {}
                Ok(true) => {
                    self.summary.deindexed += 1;
                    if self.svc.semantic.tier().is_some() {
                        self.summary.vectors_touched += 1;
                    }
                }
                Err(e) => {
                    tracing::warn!("deindex {}: {e:#}", row.paper_id);
                    self.summary.failed += 1;
                }
            }
        }
    }
}

/// A paper's body chunks, and whether deriving them changed what was stored.
struct Body {
    chunks: Vec<chunker::Chunk>,
    changed: bool,
}

/// Deriving the text and writing the Tantivy doc are separate steps: a
/// metadata or note edit needs the second without the first, and `pdftotext`
/// is by far the most expensive thing in the sweep.
async fn derive_body(
    svc: &SearchService,
    library_root: &Path,
    paper: &Paper,
    reextract: bool,
) -> Result<Body> {
    if !reextract {
        return Ok(Body {
            chunks: store::body_chunks(&svc.pool, &paper.id).await?,
            changed: false,
        });
    }
    let pdf_path = library_root.join(&paper.rel_path);
    let title = paper.meta.title.clone();
    let text = tokio::task::spawn_blocking(move || {
        crate::pdf::paper_text(&pdf_path, title.as_deref(), Pages::All)
    })
    .await
    .context("pdftotext task panicked")??;
    let chunks = chunker::body_chunks(&text);
    let changed = store::replace_body(&svc.pool, &paper.id, &chunks, &paper.content_hash).await?;
    Ok(Body { chunks, changed })
}

/// Stage the paper's Tantivy doc; returns the `doc_hash` to stamp once it is
/// committed.
async fn stage_doc(svc: &SearchService, paper: &Paper, body: &[chunker::Chunk]) -> Result<String> {
    let body: String = body
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let notes = crate::annotations::store::notes_blob(&svc.pool, &paper.id).await?;
    let doc_hash = store::doc_hash(paper, &notes);
    svc.fts_stage_upsert(fts::PaperDoc {
        id: paper.id.clone(),
        title: paper.meta.title.clone().unwrap_or_default(),
        authors: paper.meta.authors.0.join(" ; "),
        venue: paper.meta.venue.clone().unwrap_or_default(),
        abstract_text: paper.meta.abstract_text.clone().unwrap_or_default(),
        body,
        notes,
    })
    .await?;
    Ok(doc_hash)
}

async fn index_vectors(
    svc: &SearchService,
    paper: &Paper,
    body: &[chunker::Chunk],
    work: VectorWork,
) -> Result<()> {
    let Some(tier) = svc.semantic.tier() else {
        return Ok(()); // planner only schedules vectors when configured
    };
    let summary = chunker::summary_text(
        paper.meta.title.as_deref(),
        paper.meta.abstract_text.as_deref(),
    );
    let point = |seq: i64, page: Option<i64>, vector: Vec<f32>| vector::ChunkPoint {
        paper_id: paper.id.clone(),
        seq,
        page,
        vector,
    };
    let done = match work {
        VectorWork::None => return Ok(()),
        VectorWork::Summary => {
            // Unchanged text keeps its point, so a row whose stamp predates
            // `summary_hash` costs no embedding call.
            if store::embedded_summary(&svc.pool, &paper.id).await? != summary {
                let point = match &summary {
                    Some(text) => {
                        let vector = tier.embedder.embed(std::slice::from_ref(text)).await?;
                        vector.into_iter().next().map(|v| point(0, None, v))
                    }
                    None => None,
                };
                tier.store.replace_summary(&paper.id, point).await?;
            }
            VectorsDone::Summary
        }
        VectorWork::All => {
            let chunks: Vec<(i64, Option<i64>, &str)> = summary
                .iter()
                .map(|s| (0, None, s.as_str()))
                .chain(body.iter().map(|c| (c.seq, c.page, c.text.as_str())))
                .collect();
            let points = if chunks.is_empty() {
                Vec::new()
            } else {
                let texts: Vec<String> = chunks.iter().map(|(_, _, t)| t.to_string()).collect();
                let vectors = tier.embedder.embed(&texts).await?;
                chunks
                    .iter()
                    .zip(vectors)
                    .map(|(&(seq, page, _), v)| point(seq, page, v))
                    .collect()
            };
            tier.store.replace_paper(&paper.id, points).await?;
            VectorsDone::All {
                model: tier.embedder.model(),
            }
        }
    };
    store::mark_vectors_done(&svc.pool, &paper.id, summary.as_deref(), done).await
}

/// The tombstone's Tantivy doc is already gone; this deletes its points and
/// row. `false` when nothing was left for this process to do.
async fn deindex_paper(svc: &SearchService, row: &store::IndexRow) -> Result<bool> {
    let paper_id = row.paper_id.as_str();
    match svc.semantic.store() {
        None if row.embed_model.is_some() => {
            // Its points sit in a store this process cannot open (no API key
            // in this environment, or an unusable store). The row is the only
            // record that they need deleting, so it stays for a process that
            // can.
            if row.fts_indexed_at.is_none() {
                return Ok(false);
            }
            store::clear_fts_stamp(&svc.pool, paper_id).await?;
        }
        None => store::remove_index_entry(&svc.pool, paper_id).await?,
        Some(vectors) => {
            if let Err(e) = vectors.delete_paper(paper_id).await {
                // The row is the only record of the points: it stays, and the
                // vector backoff schedules the retry.
                if row.fts_indexed_at.is_some() {
                    store::clear_fts_stamp(&svc.pool, paper_id).await?;
                }
                store::record_error(&svc.pool, paper_id, &format!("{e:#}"), ErrorTier::Vectors)
                    .await?;
                return Err(e.context("deleting its vectors"));
            }
            store::remove_index_entry(&svc.pool, paper_id).await?;
        }
    }
    Ok(true)
}

/// Run the indexer loop in the background. It sweeps only while this process
/// holds the Tantivy writer — "a sweep runs" and "the writer lock is held"
/// mean the same thing, which is what lets a CLI command take the lock as
/// proof that nobody else is indexing — and waits for the lock (and for a
/// vector store another process held at startup) for as long as it takes.
/// A free writer is claimed before this returns, so a started server owns it
/// before any library change can need it.
pub(crate) async fn spawn(tasks: &Tasks, svc: Arc<SearchService>, library_root: PathBuf) {
    let claim = svc.claim_writer().await;
    tasks.spawn(run(svc, library_root, claim, tasks.token()));
}

async fn run(
    svc: Arc<SearchService>,
    library_root: PathBuf,
    claim: Result<fts::WriterClaim>,
    cancel: CancellationToken,
) {
    if !claim_writer_when_free(&svc, claim, &cancel).await {
        return;
    }
    let mut watch = match ChangeWatch::open(&svc.pool).await {
        Ok(w) => Some(w),
        Err(e) => {
            tracing::warn!("search index: not watching for other processes' changes: {e:#}");
            None
        }
    };
    loop {
        let store_waiting = svc.semantic.acquire_waiting(&svc.pool).await;
        if let Some(w) = &mut watch {
            if let Err(e) = w.mark_seen().await {
                tracing::warn!("search index: reading the database version: {e:#}");
            }
        }
        let retry_at = match sweep(&svc, &library_root).await {
            Ok(s) => {
                if !s.is_empty() {
                    tracing::info!(
                        "search index sweep: {} indexed, {} removed, {} failed",
                        s.indexed,
                        s.deindexed,
                        s.failed
                    );
                }
                s.retry_at
            }
            Err(e) => {
                tracing::warn!("search index sweep failed: {e:#}");
                None
            }
        };
        let mut idle = retry_at.map_or(IDLE_SWEEP, |at| {
            (at - Utc::now())
                .to_std()
                .unwrap_or(Duration::ZERO)
                .min(IDLE_SWEEP)
        });
        if store_waiting {
            idle = idle.min(CLAIM_RETRY);
        }
        let changed = async {
            match &mut watch {
                Some(w) => w.changed().await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            biased;
            () = cancel.cancelled() => return,
            () = svc.notify.notified() => {}
            () = changed => {}
            () = tokio::time::sleep(idle) => {}
        }
    }
}

/// `false` when cancelled first.
async fn claim_writer_when_free(
    svc: &SearchService,
    mut claim: Result<fts::WriterClaim>,
    cancel: &CancellationToken,
) -> bool {
    let mut waiting = false;
    loop {
        match claim {
            Ok(fts::WriterClaim::Claimed) => {
                if waiting {
                    tracing::info!("search indexing started: the full-text index was released");
                }
                return true;
            }
            Ok(fts::WriterClaim::Busy) if !waiting => {
                tracing::warn!("search indexing waits: {}", Skipped::IndexWriterHeld);
            }
            Err(e) if !waiting => {
                tracing::warn!("search indexing waits: taking the full-text index writer: {e:#}");
            }
            Ok(fts::WriterClaim::Busy) | Err(_) => {}
        }
        waiting = true;
        tokio::select! {
            biased;
            () = cancel.cancelled() => return false,
            () = tokio::time::sleep(CLAIM_RETRY) => {}
        }
        claim = svc.claim_writer().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::model::{Authors, Paper, PaperMeta, PaperStatus};
    use crate::search::{fts, store, vector, SearchService};
    use crate::testutil::write_pdf;
    use serde_json::json;
    use std::path::Path;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Fixture {
        svc: std::sync::Arc<SearchService>,
        library_root: std::path::PathBuf,
        _dirs: Vec<tempfile::TempDir>,
    }

    /// Temp SQLite + temp Tantivy, plus a temp vector store and wiremock
    /// embeddings when given a server.
    async fn fixture(server: Option<&MockServer>) -> Fixture {
        let db_dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", db_dir.path().join("t.db").display());
        let pool = crate::db::connect(&url).await.unwrap();
        let idx_dir = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(idx_dir.path()).unwrap();
        let lib_dir = tempfile::tempdir().unwrap();
        let library_root = lib_dir.path().to_path_buf();
        let mut dirs = vec![db_dir, idx_dir, lib_dir];
        let semantic = match server {
            Some(s) => {
                let (dir, tier) =
                    crate::testutil::tier(&format!("{}/v1", s.uri()), Vec::new()).await;
                dirs.push(dir);
                Some(tier)
            }
            None => None,
        };
        let svc = SearchService::open_with(pool, fts_idx, semantic);
        Fixture {
            svc,
            library_root,
            _dirs: dirs,
        }
    }

    async fn echo_embeddings() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(|req: &wiremock::Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                let n = body["input"].as_array().map(|a| a.len()).unwrap_or(1);
                let data: Vec<_> = (0..n)
                    .map(|i| json!({"index": i, "embedding": [0.1, 0.2, 0.3, 0.4]}))
                    .collect();
                ResponseTemplate::new(200).set_body_json(json!({"data": data}))
            })
            .mount(&server)
            .await;
        server
    }

    async fn vector_hits(store: &vector::VectorStore) -> Vec<(String, i64)> {
        store
            .search(&[0.1, 0.2, 0.3, 0.4], 100, vector::SeqFilter::All, None)
            .await
            .unwrap()
            .into_iter()
            .map(|h| (h.paper_id, h.seq))
            .collect()
    }

    async fn insert_paper_with_pdf(f: &Fixture, id: &str, title: &str, body_line: &str) {
        insert_paper_with_pdf_at(&f.svc.pool, &f.library_root, id, title, body_line).await;
    }

    async fn insert_paper_with_pdf_at(
        pool: &sqlx::SqlitePool,
        library_root: &Path,
        id: &str,
        title: &str,
        body_line: &str,
    ) {
        let rel = format!("{id}.pdf");
        write_pdf(&library_root.join(&rel), &[body_line]);
        let p = Paper {
            id: id.into(),
            content_hash: format!("hash-{id}"),
            rel_path: rel,
            cite_key: None,
            added_at: "2026-07-09T00:00:00Z".into(),
            deleted_at: None,
            starred: false,
            name: None,
            meta: PaperMeta {
                title: Some(title.into()),
                abstract_text: Some("An abstract.".into()),
                authors: Authors(vec!["Ada Lovelace".into()]),
                venue: None,
                year: Some(2026),
                doi: None,
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: PaperStatus::Resolved,
            },
        };
        crate::library::papers::insert(pool, &p).await.unwrap();
    }

    #[tokio::test]
    async fn rebuild_wipes_and_rederives_the_fts_index() {
        let f = rebuild_fixture().await;
        let ai = crate::config::AiConfig::default();
        let s = rebuild(
            f.pool.clone(),
            &f.search,
            &ai,
            &f.library_root,
            RebuildScope::Both,
        )
        .await
        .unwrap();
        assert_eq!(s.indexed, 1);

        // A second rebuild starts from a wiped directory and cleared stamps,
        // so it re-derives the same doc rather than no-opping on the stamps —
        // and proves the writer-lock probe passes once the first service is
        // gone.
        let s = rebuild(
            f.pool.clone(),
            &f.search,
            &ai,
            &f.library_root,
            RebuildScope::Both,
        )
        .await
        .unwrap();
        assert_eq!(s.indexed, 1);
    }

    #[tokio::test]
    async fn sweep_indexes_fts_even_without_embedder() {
        let f = fixture(None).await;
        insert_paper_with_pdf(
            &f,
            "p1",
            "Fuzzing Firmware",
            "the body mentions dictionaries",
        )
        .await;

        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.indexed, 1);

        let hits = f
            .svc
            .fts
            .search("dictionaries", &[], &fts::FieldSel::all(), 10, None)
            .unwrap();
        assert_eq!(hits.len(), 1, "body text searchable after sweep");
        let rows = store::all_index_rows(&f.svc.pool).await.unwrap();
        assert!(rows[0].fts_indexed_at.is_some());
        assert!(
            rows[0].vectors_indexed_at.is_none(),
            "no embedder -> no vector stamp"
        );

        // Second sweep is a no-op.
        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.indexed + s.deindexed + s.failed, 0);
    }

    /// Attach a note to a paper, the way the annotations API does.
    async fn add_note(f: &Fixture, paper_id: &str, id: &str, note: &str) {
        crate::annotations::store::upsert(
            &f.svc.pool,
            paper_id,
            id,
            &crate::annotations::NewAnnotation {
                page_index: 0,
                kind: crate::annotations::AnnotationKind::Highlight,
                color: crate::annotations::AnnotationColor::Amber,
                quoted_text: Some("some quoted sentence".into()),
                note: Some(note.into()),
                payload: json!({ "annotation": { "type": 9 } }),
            },
            "2026-08-14T00:00:00Z",
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_new_note_reindexes_fts_without_touching_the_pdf() {
        let f = fixture(None).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "the body words").await;
        assert_eq!(sweep_in(&f).await.unwrap().indexed, 1);
        assert!(f
            .svc
            .fts
            .search("bisimulation", &[], &fts::FieldSel::all(), 10, None)
            .unwrap()
            .is_empty());

        // Delete the PDF: a re-extract would now fail loudly, so a green
        // sweep proves the notes path reused the chunks already in SQLite.
        std::fs::remove_file(f.library_root.join("p1.pdf")).unwrap();
        add_note(&f, "p1", "a1", "this is really a bisimulation argument").await;

        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.indexed, 1);
        assert_eq!(s.failed, 0, "the PDF must not be re-read for a note edit");

        let mut sel = fts::FieldSel::none();
        sel.notes = true;
        let hits = f
            .svc
            .fts
            .search("bisimulation", &[], &sel, 10, None)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].paper_id, "p1");
        // The body survived the rewrite — it came from the stored chunks.
        assert_eq!(
            f.svc
                .fts
                .search("body", &[], &fts::FieldSel::all(), 10, None)
                .unwrap()
                .len(),
            1
        );

        // Third sweep is a no-op: the notes hash is stamped.
        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.indexed + s.deindexed + s.failed, 0);
        assert_eq!(f.svc.status().await.unwrap().fts.pending, 0);
    }

    #[tokio::test]
    async fn an_unstamped_note_shows_up_as_pending() {
        let f = fixture(None).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "the body words").await;
        sweep_in(&f).await.unwrap();
        add_note(&f, "p1", "a1", "worth revisiting").await;
        assert_eq!(
            f.svc.status().await.unwrap().fts.pending,
            1,
            "a note nobody has indexed yet is pending FTS work"
        );
    }

    #[tokio::test]
    async fn sweep_embeds_and_upserts_vectors_when_configured() {
        let server = echo_embeddings().await;
        let f = fixture(Some(&server)).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;

        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.indexed, 1);
        assert_eq!(s.vectors_touched, 1);
        let rows = store::all_index_rows(&f.svc.pool).await.unwrap();
        assert!(rows[0].vectors_indexed_at.is_some());
        assert_eq!(rows[0].embed_model.as_deref(), Some("m1"));
        let hits = vector_hits(f.svc.semantic.store().unwrap()).await;
        assert!(hits.contains(&("p1".to_string(), 0)), "got: {hits:?}");

        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.vectors_touched, 0, "nothing stale, nothing to optimize");
    }

    #[tokio::test]
    async fn an_fts_only_sweep_never_calls_the_embedder() {
        let server = echo_embeddings().await;
        let f = fixture(Some(&server)).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;

        let s = sweep_scoped(&f.svc, &f.library_root, Scope::FtsOnly)
            .await
            .unwrap();
        assert_eq!((s.indexed, s.vectors_touched), (1, 0));
        assert!(server.received_requests().await.unwrap().is_empty());
        let rows = store::all_index_rows(&f.svc.pool).await.unwrap();
        assert!(rows[0].fts_indexed_at.is_some());
        assert!(rows[0].vectors_indexed_at.is_none());

        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.vectors_touched, 1, "the full sweep embeds what was left");
    }

    #[tokio::test]
    async fn an_fts_only_sweep_still_deletes_a_tombstones_vectors() {
        let server = echo_embeddings().await;
        let f = fixture(Some(&server)).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;
        sweep_in(&f).await.unwrap();
        crate::library::trash::soft_delete(&f.svc.pool, "p1")
            .await
            .unwrap();

        let s = sweep_scoped(&f.svc, &f.library_root, Scope::FtsOnly)
            .await
            .unwrap();
        assert_eq!(s.deindexed, 1);
        assert!(vector_hits(f.svc.semantic.store().unwrap())
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn a_sweep_without_the_vector_store_keeps_the_tombstone_for_one_with_it() {
        let server = echo_embeddings().await;
        let f = fixture(Some(&server)).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;
        sweep_in(&f).await.unwrap();
        crate::library::trash::soft_delete(&f.svc.pool, "p1")
            .await
            .unwrap();
        f.svc.release_writer();

        let (fts_idx, _) = fts::FtsIndex::open(f._dirs[1].path()).unwrap();
        let keyless = SearchService::open_with(f.svc.pool.clone(), fts_idx, None);
        assert_eq!(sweep(&keyless, &f.library_root).await.unwrap().deindexed, 1);
        assert!(keyless
            .fts
            .search("Fuzzing", &[], &fts::FieldSel::all(), 10, None)
            .unwrap()
            .is_empty());
        let rows = store::all_index_rows(&f.svc.pool).await.unwrap();
        assert_eq!(rows.len(), 1, "the row outlives the Tantivy doc");
        assert!(rows[0].fts_indexed_at.is_none());
        assert!(sweep(&keyless, &f.library_root).await.unwrap().is_empty());
        keyless.release_writer();

        assert_eq!(sweep_in(&f).await.unwrap().deindexed, 1);
        assert!(store::all_index_rows(&f.svc.pool).await.unwrap().is_empty());
        assert!(vector_hits(f.svc.semantic.store().unwrap())
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn an_fts_only_extraction_failure_leaves_the_vector_backoff_alone() {
        let server = echo_embeddings().await;
        let f = fixture(Some(&server)).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;
        std::fs::write(f.library_root.join("p1.pdf"), b"not a pdf").unwrap();

        let s = sweep_scoped(&f.svc, &f.library_root, Scope::FtsOnly)
            .await
            .unwrap();
        assert_eq!(s.failed, 1);
        let rows = store::all_index_rows(&f.svc.pool).await.unwrap();
        assert_eq!((rows[0].fts_attempts, rows[0].vec_attempts), (1, 0));
    }

    #[tokio::test]
    async fn catch_up_skips_an_index_another_writer_holds() {
        let f = fixture(None).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;
        let (holder, _) = fts::FtsIndex::open(f._dirs[1].path()).unwrap();
        assert_eq!(holder.claim_writer().unwrap(), fts::WriterClaim::Claimed);

        let out = catch_up(&f.svc, &f.library_root, Scope::Full)
            .await
            .unwrap();
        assert!(matches!(out, CatchUp::Skipped(Skipped::IndexWriterHeld)));
        assert!(
            store::all_index_rows(&f.svc.pool).await.unwrap().is_empty(),
            "a skipped sweep records no failures"
        );

        drop(holder);
        let out = catch_up(&f.svc, &f.library_root, Scope::Full)
            .await
            .unwrap();
        assert!(matches!(out, CatchUp::Swept(s) if s.indexed == 1));
    }

    #[tokio::test]
    async fn catch_up_skips_a_vector_store_another_process_holds() {
        let dir = tempfile::tempdir().unwrap();
        let (dir_db, pool) = crate::testutil::temp_pool().await;
        let cfg = crate::config::SearchConfig {
            index_dir: dir.path().join("index"),
            vector_dir: dir.path().join("vectors"),
        };
        let semantic = Semantic::Unavailable(OpenError::Locked {
            dir: cfg.vector_dir.clone(),
            holder: None,
        });
        let svc = SearchService::open(pool, &cfg, semantic).await.unwrap();

        let out = catch_up(&svc, dir_db.path(), Scope::FtsOnly).await.unwrap();
        assert!(matches!(out, CatchUp::Skipped(Skipped::VectorStoreHeld)));
        let (other, _) = fts::FtsIndex::open(&cfg.index_dir).unwrap();
        assert_eq!(
            other.claim_writer().unwrap(),
            fts::WriterClaim::Claimed,
            "skipping must not take the writer either"
        );
    }

    async fn eventually(what: &str, mut done: impl AsyncFnMut() -> bool) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        while !done().await {
            assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn fts_indexed(svc: &SearchService) -> i64 {
        svc.status().await.unwrap().fts.indexed
    }

    #[tokio::test]
    async fn the_loop_waits_for_a_held_writer_then_follows_every_change() {
        let f = fixture(None).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;
        let (holder, _) = fts::FtsIndex::open(f._dirs[1].path()).unwrap();
        holder.claim_writer().unwrap();

        let tasks = Tasks::new();
        spawn(&tasks, f.svc.clone(), f.library_root.clone()).await;
        tokio::time::sleep(CLAIM_RETRY + Duration::from_millis(500)).await;
        assert_eq!(fts_indexed(&f.svc).await, 0, "no sweep without the writer");

        drop(holder);
        eventually("the loop takes the writer once released", async || {
            fts_indexed(&f.svc).await == 1
        })
        .await;
        let (other, _) = fts::FtsIndex::open(f._dirs[1].path()).unwrap();
        assert_eq!(other.claim_writer().unwrap(), fts::WriterClaim::Busy);

        // A paper added through another connection, the way a CLI command
        // beside the server adds one, with nobody calling `wake`.
        let elsewhere = crate::db::connect(&format!(
            "sqlite:{}",
            f._dirs[0].path().join("t.db").display()
        ))
        .await
        .unwrap();
        insert_paper_with_pdf_at(&elsewhere, &f.library_root, "p2", "Sorting", "words").await;
        eventually("a commit by another process is noticed", async || {
            fts_indexed(&f.svc).await == 2
        })
        .await;
        tasks.shutdown(Duration::from_secs(5)).await;
    }

    #[tokio::test]
    async fn a_rechunk_with_fewer_chunks_leaves_no_stale_points() {
        let server = echo_embeddings().await;
        let f = fixture(Some(&server)).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "x").await;
        write_pdf(&f.library_root.join("p1.pdf"), &["page one", "page two"]);
        sweep_in(&f).await.unwrap();
        let mut hits = vector_hits(f.svc.semantic.store().unwrap()).await;
        hits.sort();
        assert_eq!(hits, [("p1".into(), 0), ("p1".into(), 1), ("p1".into(), 2)]);

        write_pdf(&f.library_root.join("p1.pdf"), &["one page now"]);
        sqlx::query("UPDATE papers SET content_hash = 'hash-p1-v2' WHERE id = 'p1'")
            .execute(&f.svc.pool)
            .await
            .unwrap();
        assert_eq!(sweep_in(&f).await.unwrap().indexed, 1);
        let mut hits = vector_hits(f.svc.semantic.store().unwrap()).await;
        hits.sort();
        assert_eq!(hits, [("p1".into(), 0), ("p1".into(), 1)]);
    }

    async fn embedded_texts(server: &MockServer) -> Vec<Vec<String>> {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|r| {
                let body: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
                serde_json::from_value(body["input"].clone()).unwrap()
            })
            .collect()
    }

    #[tokio::test]
    async fn metadata_edits_redo_only_what_reads_the_field() {
        let server = echo_embeddings().await;
        let f = fixture(Some(&server)).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;
        sweep_in(&f).await.unwrap();
        assert_eq!(embedded_texts(&server).await.len(), 1);
        // A re-extract would now fail: every pass below reuses stored chunks.
        std::fs::remove_file(f.library_root.join("p1.pdf")).unwrap();
        let edit = async |sql: &'static str| {
            sqlx::query(sql).execute(&f.svc.pool).await.unwrap();
            let s = sweep_in(&f).await.unwrap();
            assert_eq!(s.failed, 0, "{sql}");
            s
        };

        // Year feeds no index.
        let s = edit("UPDATE papers SET year = 1999 WHERE id = 'p1'").await;
        assert!(s.is_empty());

        // Venue is in the Tantivy doc only.
        let s = edit("UPDATE papers SET venue = 'Usenix Security' WHERE id = 'p1'").await;
        assert_eq!((s.indexed, s.vectors_touched), (1, 0));
        assert_eq!(embedded_texts(&server).await.len(), 1);
        assert_eq!(
            f.svc
                .fts
                .search("venue:usenix", &[], &fts::FieldSel::all(), 10, None)
                .unwrap()
                .len(),
            1
        );

        // A title edit re-embeds the seq-0 chunk alone.
        let s = edit("UPDATE papers SET title = 'Taming Firmware' WHERE id = 'p1'").await;
        assert_eq!((s.indexed, s.vectors_touched), (1, 1));
        let texts = embedded_texts(&server).await;
        assert_eq!(texts.len(), 2);
        assert_eq!(texts[1], ["Taming Firmware\nAn abstract."]);
        assert_eq!(
            store::embedded_summary(&f.svc.pool, "p1")
                .await
                .unwrap()
                .as_deref(),
            Some("Taming Firmware\nAn abstract.")
        );
        assert_eq!(
            f.svc
                .fts
                .search("taming", &[], &fts::FieldSel::all(), 10, None)
                .unwrap()
                .len(),
            1
        );
        assert!(sweep_in(&f).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_summary_stamp_from_before_the_split_costs_no_embedding() {
        let server = echo_embeddings().await;
        let f = fixture(Some(&server)).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;
        sweep_in(&f).await.unwrap();
        // What migration 0026 leaves behind.
        sqlx::query("UPDATE search_index SET summary_hash = '', doc_hash = ''")
            .execute(&f.svc.pool)
            .await
            .unwrap();
        std::fs::remove_file(f.library_root.join("p1.pdf")).unwrap();

        let s = sweep_in(&f).await.unwrap();
        assert_eq!((s.indexed, s.failed), (1, 0));
        assert_eq!(
            embedded_texts(&server).await.len(),
            1,
            "no second embedding"
        );
        assert!(sweep_in(&f).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn one_sweep_commits_its_docs_in_batches() {
        let f = fixture(None).await;
        for i in 0..5 {
            insert_paper_with_pdf(&f, &format!("p{i}"), "Fuzzing Firmware", "body words").await;
        }
        let before = f.svc.fts.commits();
        assert_eq!(sweep_in(&f).await.unwrap().indexed, 5);
        assert_eq!(
            f.svc.fts.commits() - before,
            1,
            "one commit, not one per paper"
        );
        assert_eq!(f.svc.status().await.unwrap().fts.indexed, 5);
    }

    #[tokio::test]
    async fn embedding_failure_keeps_fts_and_records_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(400))
            .mount(&server)
            .await;

        let f = fixture(Some(&server)).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;

        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.failed, 1);
        let rows = store::all_index_rows(&f.svc.pool).await.unwrap();
        assert!(rows[0].fts_indexed_at.is_some(), "FTS tier survived");
        assert!(rows[0].vectors_indexed_at.is_none());
        // The failure is charged to the vector tier alone: the FTS tier
        // succeeded and must carry no backoff.
        assert!(rows[0].vec_last_error.is_some());
        assert_eq!(rows[0].vec_attempts, 1);
        assert!(rows[0].fts_last_error.is_none());
        assert_eq!(rows[0].fts_attempts, 0);
        let st = f.svc.status().await.unwrap();
        assert_eq!(st.fts.failed, 0);
        assert_eq!(st.vectors.failed, 1);
    }

    #[tokio::test]
    async fn a_backed_off_embedder_does_not_delay_a_note_reindex() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(400))
            .mount(&server)
            .await;

        let f = fixture(Some(&server)).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;
        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.failed, 1, "embedding fails; FTS lands");

        // The very next sweep, while the vector tier is still backing off:
        // a fresh note's FTS rewrite must go through immediately.
        add_note(&f, "p1", "a1", "note about bisimulation").await;
        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.indexed, 1, "FTS reindexed despite the vector backoff");
        assert_eq!(s.failed, 0, "the backed-off vector tier was not retried");

        let mut sel = fts::FieldSel::none();
        sel.notes = true;
        assert_eq!(
            f.svc
                .fts
                .search("bisimulation", &[], &sel, 10, None)
                .unwrap()
                .len(),
            1
        );
        let rows = store::all_index_rows(&f.svc.pool).await.unwrap();
        assert_eq!(
            rows[0].vec_attempts, 1,
            "the FTS success must not reset the vector tier's backoff"
        );
    }

    #[tokio::test]
    async fn extraction_failure_on_new_paper_records_error_and_backs_off() {
        let f = fixture(None).await;
        // A paper row whose PDF was never written (missing/corrupt file):
        // extraction fails before replace_chunks ever creates a search_index
        // row, so record_error must upsert one rather than no-op.
        let p = Paper {
            id: "p1".into(),
            content_hash: "hash-p1".into(),
            rel_path: "does-not-exist.pdf".into(),
            cite_key: None,
            added_at: "2026-07-09T00:00:00Z".into(),
            deleted_at: None,
            starred: false,
            name: None,
            meta: PaperMeta {
                title: Some("Fuzzing Firmware".into()),
                abstract_text: Some("An abstract.".into()),
                authors: Authors(vec!["Ada Lovelace".into()]),
                venue: None,
                year: Some(2026),
                doi: None,
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: PaperStatus::Resolved,
            },
        };
        crate::library::papers::insert(&f.svc.pool, &p)
            .await
            .unwrap();

        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.failed, 1);
        let rows = store::all_index_rows(&f.svc.pool).await.unwrap();
        assert_eq!(rows.len(), 1, "record_error must create the missing row");
        assert_eq!(rows[0].fts_attempts, 1);
        assert!(rows[0].fts_last_error.is_some());
        // No embedder: the vector tier never runs, so charging it would
        // leave an error nothing ever clears.
        assert_eq!(rows[0].vec_attempts, 0);
        assert!(rows[0].vec_last_error.is_none());
        let st = f.svc.status().await.unwrap();
        assert_eq!(st.fts.failed, 1);
        assert_eq!(st.vectors.failed, 0);

        // Second sweep right away: backoff suppresses the retry.
        let s = sweep_in(&f).await.unwrap();
        assert_eq!(
            s.indexed + s.deindexed + s.failed,
            0,
            "backoff should suppress an immediate retry"
        );
    }

    #[tokio::test]
    async fn extraction_failure_with_an_embedder_backs_off_both_tiers() {
        let server = MockServer::start().await;
        let f = fixture(Some(&server)).await;
        // Missing PDF: the shared precursor fails, and with an embedder
        // configured neither tier may hot-loop on it.
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;
        std::fs::remove_file(f.library_root.join("p1.pdf")).unwrap();

        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.failed, 1);
        let rows = store::all_index_rows(&f.svc.pool).await.unwrap();
        assert_eq!(rows[0].fts_attempts, 1);
        assert_eq!(rows[0].vec_attempts, 1);
        assert!(rows[0].fts_last_error.is_some());
        assert!(rows[0].vec_last_error.is_some());
    }

    #[tokio::test]
    async fn trashed_paper_is_deindexed_everywhere() {
        let f = fixture(None).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;
        sweep_in(&f).await.unwrap();
        assert_eq!(
            f.svc
                .fts
                .search("fuzzing", &[], &fts::FieldSel::all(), 10, None)
                .unwrap()
                .len(),
            1
        );

        crate::library::trash::soft_delete(&f.svc.pool, "p1")
            .await
            .unwrap();
        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.deindexed, 1);
        assert!(f
            .svc
            .fts
            .search("fuzzing", &[], &fts::FieldSel::all(), 10, None)
            .unwrap()
            .is_empty());
        assert!(store::all_index_rows(&f.svc.pool).await.unwrap().is_empty());
        assert!(store::body_chunks(&f.svc.pool, "p1")
            .await
            .unwrap()
            .is_empty());
        // Vector delete for a no-embedder service is skipped, not an error.
    }

    #[tokio::test]
    async fn trashed_paper_vectors_are_removed_from_the_store() {
        let server = echo_embeddings().await;
        let f = fixture(Some(&server)).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;
        insert_paper_with_pdf(&f, "p2", "Sorting Networks", "other words").await;
        sweep_in(&f).await.unwrap();

        crate::library::trash::soft_delete(&f.svc.pool, "p1")
            .await
            .unwrap();
        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.deindexed, 1);
        let hits = vector_hits(f.svc.semantic.store().unwrap()).await;
        assert!(hits.iter().all(|(id, _)| id == "p2"), "got: {hits:?}");
        assert!(!hits.is_empty());
    }

    #[tokio::test]
    async fn a_failed_vector_delete_keeps_the_tombstone_until_a_retry_succeeds() {
        let server = echo_embeddings().await;
        let f = fixture(Some(&server)).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;
        sweep_in(&f).await.unwrap();

        f.svc.semantic.store().unwrap().close().await.unwrap();
        crate::library::trash::soft_delete(&f.svc.pool, "p1")
            .await
            .unwrap();
        let s = sweep_in(&f).await.unwrap();
        assert_eq!((s.deindexed, s.failed), (0, 1));
        assert!(f
            .svc
            .fts
            .search("fuzzing", &[], &fts::FieldSel::all(), 10, None)
            .unwrap()
            .is_empty());
        let rows = store::all_index_rows(&f.svc.pool).await.unwrap();
        assert_eq!(rows.len(), 1, "the row is the only record of the points");
        assert!(rows[0].fts_indexed_at.is_none());
        assert_eq!(rows[0].vec_attempts, 1);
        assert!(sweep_in(&f).await.unwrap().is_empty(), "backed off");

        // Once the backoff elapses, a process with a working store finishes.
        f.svc.release_writer();
        sqlx::query("UPDATE search_index SET vec_last_attempt_at = '2000-01-01T00:00:00Z'")
            .execute(&f.svc.pool)
            .await
            .unwrap();
        let vector_dir = f._dirs[3].path().join("vectors");
        let (vectors, _) = vector::VectorStore::open(&vector_dir, 4, OpenMode::LoadOrCreate)
            .await
            .unwrap();
        assert!(!vector_hits(&vectors).await.is_empty());
        let (fts_idx, _) = fts::FtsIndex::open(f._dirs[1].path()).unwrap();
        let retry = SearchService::open_with(
            f.svc.pool.clone(),
            fts_idx,
            Some(crate::search::SemanticTier {
                embedder: crate::search::embedder::Embedder::for_tests(
                    &format!("{}/v1", server.uri()),
                    "m1",
                    4,
                ),
                store: vectors,
            }),
        );
        assert_eq!(sweep(&retry, &f.library_root).await.unwrap().deindexed, 1);
        assert!(store::all_index_rows(&f.svc.pool).await.unwrap().is_empty());
        assert!(vector_hits(retry.semantic.store().unwrap())
            .await
            .is_empty());
    }

    struct RebuildFixture {
        pool: sqlx::SqlitePool,
        search: crate::config::SearchConfig,
        library_root: std::path::PathBuf,
        _dir: tempfile::TempDir,
    }

    async fn rebuild_fixture() -> RebuildFixture {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let pool = crate::db::connect(&url).await.unwrap();
        let library_root = dir.path().join("library");
        std::fs::create_dir_all(&library_root).unwrap();
        insert_paper_with_pdf_at(&pool, &library_root, "p1", "Fuzzing Firmware", "body words")
            .await;
        let search = crate::config::SearchConfig {
            index_dir: dir.path().join("search-index"),
            vector_dir: dir.path().join("search-vectors"),
        };
        RebuildFixture {
            pool,
            search,
            library_root,
            _dir: dir,
        }
    }

    #[tokio::test]
    async fn rebuild_refuses_a_held_vector_store_before_wiping_the_fts_index() {
        let server = echo_embeddings().await;
        let ai = crate::testutil::embedding_ai(&format!("{}/v1", server.uri()), 4);
        let f = rebuild_fixture().await;
        rebuild(
            f.pool.clone(),
            &f.search,
            &ai,
            &f.library_root,
            RebuildScope::Both,
        )
        .await
        .unwrap();
        let index_meta = f.search.index_dir.join("meta.json");
        assert!(index_meta.exists());

        let (_held, _) = vector::VectorStore::open(&f.search.vector_dir, 4, OpenMode::LoadOrCreate)
            .await
            .unwrap();
        for scope in [RebuildScope::Fts, RebuildScope::Vectors, RebuildScope::Both] {
            let err = rebuild(f.pool.clone(), &f.search, &ai, &f.library_root, scope)
                .await
                .unwrap_err();
            assert!(err.to_string().contains("in use"), "got: {err}");
            assert!(index_meta.exists(), "the Tantivy index must survive");
        }
    }

    #[tokio::test]
    async fn rebuild_vectors_only_recreates_a_store_with_other_dims() {
        let server = echo_embeddings().await;
        let f = rebuild_fixture().await;
        let (old, _) = vector::VectorStore::open(&f.search.vector_dir, 8, OpenMode::LoadOrCreate)
            .await
            .unwrap();
        old.close().await.unwrap();

        let ai = crate::testutil::embedding_ai(&format!("{}/v1", server.uri()), 4);
        let s = rebuild(
            f.pool.clone(),
            &f.search,
            &ai,
            &f.library_root,
            RebuildScope::Vectors,
        )
        .await
        .unwrap();
        assert_eq!((s.indexed, s.failed), (1, 0));

        let (store, opened) =
            vector::VectorStore::open(&f.search.vector_dir, 4, OpenMode::LoadOrCreate)
                .await
                .unwrap();
        assert_eq!(
            opened,
            vector::Opened::Populated,
            "rebuild released the store"
        );
        assert!(vector_hits(&store).await.contains(&("p1".to_string(), 0)));
    }

    // Helper used by every test: sweep against the fixture's library root.
    async fn sweep_in(f: &Fixture) -> anyhow::Result<SweepSummary> {
        sweep(&f.svc, &f.library_root).await
    }
}
