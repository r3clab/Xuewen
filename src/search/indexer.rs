use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::search::vector::{OpenError, OpenMode};
use crate::search::{chunker, fts, planner, store, vector, SearchService, Semantic};
use crate::tasks::Tasks;

#[derive(Debug, Default)]
pub struct SweepSummary {
    pub indexed: usize,
    pub(crate) deindexed: usize,
    pub failed: usize,
    pub(crate) vectors_touched: usize,
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

/// How long an indexer owner waits for the writer at startup: long enough
/// to outlast a CLI command's catch-up sweep that happens to overlap it.
pub(crate) const CLAIM_WAIT: Duration = Duration::from_secs(10);

/// Drop and re-derive the search indexes from SQLite + PDFs (`xuewen index
/// rebuild`). Nothing is wiped until both indexes are known to be ours:
/// `probe_writable` refuses a Tantivy index another process is writing, and
/// the vector store's lock is taken (and, for a vectors rebuild, the store
/// recreated under it) before the Tantivy directory goes. `SearchService::open`
/// then detects the fresh Tantivy directory and clears the FTS stamps itself,
/// a recreated store clears the vector stamps, and one full sweep follows.
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
    if do_fts {
        fts::FtsIndex::probe_writable(&search.index_dir)?;
    }
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
    if do_fts {
        match std::fs::remove_dir_all(&search.index_dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => anyhow::bail!(
                "could not clear search index dir {}: {e}",
                search.index_dir.display()
            ),
        }
    }
    let svc = SearchService::open(pool.clone(), search, semantic).await?;
    if svc.claim_writer(Duration::ZERO).await? == fts::WriterClaim::Busy {
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
    if svc.claim_writer(Duration::ZERO).await? == fts::WriterClaim::Busy {
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
        chrono::Utc::now(),
    );
    let mut summary = SweepSummary::default();

    for row in plan
        .deindex
        .iter()
        .filter_map(|id| rows_by_id.get(id.as_str()))
    {
        let paper_id = &row.paper_id;
        match deindex_paper(svc, row).await {
            Ok(false) => {}
            Ok(true) => {
                summary.deindexed += 1;
                if tier.is_some() {
                    summary.vectors_touched += 1;
                }
            }
            Err(e) => {
                tracing::warn!("deindex {paper_id}: {e}");
                summary.failed += 1;
            }
        }
    }
    for work in &plan.index {
        match index_paper(svc, library_root, work, scope).await {
            Ok(true) => {
                summary.indexed += 1;
                if work.vectors {
                    summary.vectors_touched += 1;
                }
            }
            Ok(false) => {}
            Err(f) => {
                tracing::warn!("index {}: {}", work.paper_id, f.source);
                store::record_error(&svc.pool, &work.paper_id, &f.source.to_string(), f.tier)
                    .await
                    .ok();
                summary.failed += 1;
            }
        }
    }
    if let Some(tier) = tier.filter(|_| summary.vectors_touched > 0) {
        if let Err(e) = tier.store.optimize().await {
            tracing::warn!("vector store optimize: {e:#}");
        }
    }
    Ok(summary)
}

/// A paper-indexing failure, tagged with the tier whose backoff it charges
/// so a broken embedder never delays healthy FTS work (or vice versa).
struct IndexFailure {
    tier: store::ErrorTier,
    source: anyhow::Error,
}

fn charge(tier: store::ErrorTier) -> impl Fn(anyhow::Error) -> IndexFailure {
    move |source| IndexFailure { tier, source }
}

async fn index_paper(
    svc: &SearchService,
    library_root: &Path,
    work: &planner::Work,
    scope: Scope,
) -> Result<bool, IndexFailure> {
    // Failures before the tiers split (lookup, extraction, chunking) charge
    // BOTH: the broken precursor serves both tiers, so neither may hot-loop
    // on it. Without an embedder there is no vector tier to hot-loop — and a
    // `vec_last_error` written then would never be cleared, since no vector
    // pass ever runs to clear it — so the charge narrows to FTS alone. An
    // FTS-only pass narrows it too: it never ran the vector tier.
    let pre_tier = if scope == Scope::Full && svc.semantic.tier().is_some() {
        store::ErrorTier::Both
    } else {
        store::ErrorTier::Fts
    };
    let Some(paper) = crate::library::papers::get(&svc.pool, &work.paper_id)
        .await
        .map_err(charge(pre_tier))?
    else {
        return Ok(false); // purged since the plan was computed; tombstone next sweep
    };

    let chunks = derive_chunks(svc, library_root, &paper, work.reextract)
        .await
        .map_err(charge(pre_tier))?;

    if work.fts {
        index_fts(svc, &paper, &chunks)
            .await
            .map_err(charge(store::ErrorTier::Fts))?;
    }
    if work.vectors {
        index_vectors(svc, &paper, &chunks)
            .await
            .map_err(charge(store::ErrorTier::Vectors))?;
    }
    Ok(true)
}

/// Deriving the text and writing the Tantivy doc are separate steps: a note
/// edit needs the second without the first, and `pdftotext` is by far the
/// most expensive thing in the sweep.
async fn derive_chunks(
    svc: &SearchService,
    library_root: &Path,
    paper: &crate::library::model::Paper,
    reextract: bool,
) -> Result<Vec<chunker::Chunk>> {
    if !reextract {
        return store::chunks_for_paper(&svc.pool, &paper.id).await;
    }
    let pdf_path = library_root.join(&paper.rel_path);
    let text = tokio::task::spawn_blocking(move || crate::pdf::extract_text_all(&pdf_path))
        .await
        .context("pdftotext task panicked")??;
    let chunks = chunker::chunk_paper(
        paper.meta.title.as_deref(),
        paper.meta.abstract_text.as_deref(),
        &text,
    );
    store::replace_chunks(
        &svc.pool,
        &paper.id,
        &chunks,
        &paper.content_hash,
        &store::meta_hash(paper),
    )
    .await?;
    Ok(chunks)
}

async fn index_fts(
    svc: &SearchService,
    paper: &crate::library::model::Paper,
    chunks: &[chunker::Chunk],
) -> Result<()> {
    let body: String = chunks
        .iter()
        .filter(|c| c.seq >= 1)
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let notes = crate::annotations::store::notes_blob(&svc.pool, &paper.id).await?;
    let notes_hash = store::notes_hash(&notes);
    svc.fts_upsert(fts::PaperDoc {
        id: paper.id.clone(),
        title: paper.meta.title.clone().unwrap_or_default(),
        authors: paper.meta.authors.0.join(" ; "),
        venue: paper.meta.venue.clone().unwrap_or_default(),
        abstract_text: paper.meta.abstract_text.clone().unwrap_or_default(),
        body,
        notes,
    })
    .await?;
    store::mark_fts_done(&svc.pool, &paper.id, &notes_hash).await?;
    Ok(())
}

async fn index_vectors(
    svc: &SearchService,
    paper: &crate::library::model::Paper,
    chunks: &[chunker::Chunk],
) -> Result<()> {
    let Some(tier) = svc.semantic.tier() else {
        return Ok(()); // planner only schedules vectors when configured
    };
    if !chunks.is_empty() {
        let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
        let vectors = tier.embedder.embed(&texts).await?;
        let points: Vec<vector::ChunkPoint> = chunks
            .iter()
            .zip(vectors)
            .map(|(c, v)| vector::ChunkPoint {
                paper_id: paper.id.clone(),
                seq: c.seq,
                page: c.page,
                vector: v,
            })
            .collect();
        tier.store.upsert(points).await?;
    }
    store::mark_vectors_done(&svc.pool, &paper.id, tier.embedder.model()).await?;
    Ok(())
}

/// `false` when nothing was left for this process to do.
async fn deindex_paper(svc: &SearchService, row: &store::IndexRow) -> Result<bool> {
    let paper_id = row.paper_id.as_str();
    let tier = svc.semantic.tier();
    if tier.is_none() && row.embed_model.is_some() {
        // Its points may sit in a store this process cannot open (no API key
        // in this environment, or an unusable store). The row is the only
        // record that they need deleting, so it stays for a process that can;
        // only the Tantivy doc goes, once.
        if row.fts_indexed_at.is_none() {
            return Ok(false);
        }
        svc.fts_delete(paper_id.to_string()).await?;
        store::clear_fts_stamp(&svc.pool, paper_id).await?;
        return Ok(true);
    }
    svc.fts_delete(paper_id.to_string()).await?;
    if let Some(tier) = tier {
        // Vector cleanup only matters when vectors were ever written; a failing
        // store here must not wedge the tombstone forever.
        if let Err(e) = tier.store.delete_paper(paper_id).await {
            tracing::warn!(
                "vector delete {paper_id}: {e} (index row removed anyway; \
                            orphan points are overwritten if the paper returns)"
            );
        }
    }
    store::remove_index_entry(&svc.pool, paper_id).await?;
    Ok(true)
}

/// Start the indexer loop, but only once this process holds the Tantivy
/// writer: "an indexer loop runs" and "the writer lock is held" then mean the
/// same thing, which is what lets a CLI command take the lock as proof that
/// nobody else is indexing. On `Busy` no loop starts.
pub(crate) async fn spawn(
    tasks: &Tasks,
    svc: Arc<SearchService>,
    library_root: PathBuf,
) -> Result<fts::WriterClaim> {
    spawn_claiming(tasks, svc, library_root, CLAIM_WAIT).await
}

async fn spawn_claiming(
    tasks: &Tasks,
    svc: Arc<SearchService>,
    library_root: PathBuf,
    wait: Duration,
) -> Result<fts::WriterClaim> {
    let claim = svc.claim_writer(wait).await?;
    match claim {
        fts::WriterClaim::Claimed => tasks.spawn(run(
            svc,
            library_root,
            Duration::from_secs(30),
            tasks.token(),
        )),
        fts::WriterClaim::Busy => tracing::warn!(
            "search indexing disabled: the full-text index is held by another xuewen process"
        ),
    }
    Ok(claim)
}

/// Indexer loop: sweep, then sleep until woken or the tick elapses.
async fn run(
    svc: Arc<SearchService>,
    library_root: PathBuf,
    tick: Duration,
    cancel: CancellationToken,
) {
    loop {
        match sweep(&svc, &library_root).await {
            Ok(s) if !s.is_empty() => {
                tracing::info!(
                    "search index sweep: {} indexed, {} removed, {} failed",
                    s.indexed,
                    s.deindexed,
                    s.failed
                );
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("search index sweep failed: {e}"),
        }
        tokio::select! {
            biased;
            () = cancel.cancelled() => return,
            () = svc.wait_work(tick) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::model::{Authors, Paper, PaperMeta, PaperStatus};
    use crate::search::{fts, store, vector, SearchService};
    use printpdf::*;
    use serde_json::json;
    use std::path::Path;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn write_pdf(path: &Path, line: &str) {
        let mut doc = PdfDocument::new("t");
        let ops = vec![
            Op::StartTextSection,
            Op::SetFont {
                font: PdfFontHandle::Builtin(BuiltinFont::Helvetica),
                size: Pt(12.0),
            },
            Op::SetTextCursor {
                pos: Point::new(Mm(15.0), Mm(280.0)),
            },
            Op::ShowText {
                items: vec![TextItem::Text(line.to_string())],
            },
            Op::EndTextSection,
        ];
        let page = PdfPage::new(Mm(210.0), Mm(297.0), ops);
        let bytes = doc
            .with_pages(vec![page])
            .save(&PdfSaveOptions::default(), &mut Vec::new());
        std::fs::write(path, bytes).unwrap();
    }

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
        let dirs = vec![db_dir, idx_dir, lib_dir];
        let semantic = match server {
            Some(s) => Some(crate::testutil::tier(&format!("{}/v1", s.uri()), Vec::new()).await),
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
        write_pdf(&library_root.join(&rel), body_line);
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
            .search("dictionaries", &fts::FieldSel::all(), 10, None)
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
            .search("bisimulation", &fts::FieldSel::all(), 10, None)
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
        let hits = f.svc.fts.search("bisimulation", &sel, 10, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].paper_id, "p1");
        // The body survived the rewrite — it came from the stored chunks.
        assert_eq!(
            f.svc
                .fts
                .search("body", &fts::FieldSel::all(), 10, None)
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
            .search("Fuzzing", &fts::FieldSel::all(), 10, None)
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

    #[tokio::test]
    async fn spawn_holds_the_writer_and_refuses_to_start_beside_a_holder() {
        let f = fixture(None).await;
        let tasks = Tasks::new();
        let claim = spawn(&tasks, f.svc.clone(), f.library_root.clone())
            .await
            .unwrap();
        assert_eq!(claim, fts::WriterClaim::Claimed);
        let (other, _) = fts::FtsIndex::open(f._dirs[1].path()).unwrap();
        assert_eq!(other.claim_writer().unwrap(), fts::WriterClaim::Busy);
        tasks.shutdown(Duration::from_secs(5)).await;

        let g = fixture(None).await;
        let (holder, _) = fts::FtsIndex::open(g._dirs[1].path()).unwrap();
        holder.claim_writer().unwrap();
        let claim = spawn_claiming(
            &tasks,
            g.svc.clone(),
            g.library_root.clone(),
            Duration::ZERO,
        )
        .await
        .unwrap();
        assert_eq!(claim, fts::WriterClaim::Busy);
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
                .search("bisimulation", &sel, 10, None)
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
                .search("fuzzing", &fts::FieldSel::all(), 10, None)
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
            .search("fuzzing", &fts::FieldSel::all(), 10, None)
            .unwrap()
            .is_empty());
        assert!(store::all_index_rows(&f.svc.pool).await.unwrap().is_empty());
        assert!(store::chunks_for_paper(&f.svc.pool, "p1")
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
    async fn vector_delete_failure_does_not_wedge_tombstone() {
        let server = echo_embeddings().await;
        let f = fixture(Some(&server)).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;
        sweep_in(&f).await.unwrap();

        f.svc.semantic.store().unwrap().close().await.unwrap();
        crate::library::trash::soft_delete(&f.svc.pool, "p1")
            .await
            .unwrap();
        let s = sweep_in(&f).await.unwrap();
        assert_eq!(
            s.deindexed, 1,
            "tombstone cleared despite the store failing"
        );
        assert!(store::all_index_rows(&f.svc.pool).await.unwrap().is_empty());
        assert!(f
            .svc
            .fts
            .search("fuzzing", &fts::FieldSel::all(), 10, None)
            .unwrap()
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
