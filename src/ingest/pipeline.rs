use anyhow::{Context as _, Result};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use super::fields::{paper_meta, recover_unique_collision, resolve_fields, ResolveInputs};
use crate::config::Config;
use crate::http::RetryPolicy;
use crate::library::model::{Identifier, Paper};
use crate::library::naming;
use crate::library::naming::files::{
    copy_to_async, copy_to_excl_async, file_hash_matches, is_already_exists, move_to_async,
};
use crate::library::papers;
use crate::metadata::identify;
use crate::metadata::resolve::grobid::Grobid;
use crate::metadata::resolve::{ResolvedMetadata, Resolver, TitleQuery};
use crate::{db, hash, pdf};

/// Directories the pipeline manages. `under` is the one place the managed
/// inbox subdirectory names (`_processed`, `_uploads`, `_failed`) are spelled.
pub struct Libraries {
    pub(crate) library_root: PathBuf,
    /// Where ingested originals are archived out of the inbox.
    processed_dir: PathBuf,
    /// Where fetched/uploaded bytes are staged before ingest (`ingest_bytes`).
    pub(crate) staging_dir: PathBuf,
    /// Where the watcher quarantines PDFs that repeatedly fail to ingest.
    pub(crate) failed_dir: PathBuf,
}

impl Libraries {
    /// The managed directories under an inbox and a library root.
    pub fn under(inbox_dir: &Path, library_root: &Path) -> Self {
        Self {
            library_root: library_root.to_path_buf(),
            processed_dir: inbox_dir.join("_processed"),
            staging_dir: inbox_dir.join("_uploads"),
            failed_dir: inbox_dir.join("_failed"),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Ingested(String),  // new paper id
    Duplicate(String), // same bytes as an active paper → its id
    SameWork(String),  // same DOI/arXiv id as an active paper → its id
    InTrash(String),   // same bytes or identifier as a trashed paper → its id
}

impl Outcome {
    pub(crate) fn from_hash_hit(existing: &Paper) -> Self {
        if existing.deleted_at.is_some() {
            Outcome::InTrash(existing.id.clone())
        } else {
            Outcome::Duplicate(existing.id.clone())
        }
    }

    pub(crate) fn from_identifier_hit(existing: &Paper) -> Self {
        if existing.deleted_at.is_some() {
            Outcome::InTrash(existing.id.clone())
        } else {
            Outcome::SameWork(existing.id.clone())
        }
    }
}

/// What happens to the source file once ingest reaches an outcome. An ingest
/// *error* always leaves the source in place for the caller to handle (the
/// watcher retries/quarantines, `ingest_bytes` deletes its staged copy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposal {
    /// Archive into `_processed`: real user files (inbox watcher, CLI ingest).
    Archive,
    /// Delete: app-created staged copies (`ingest_bytes`), whose bytes ingest
    /// has already filed into the library — archiving those too would store
    /// every upload/URL import twice forever.
    Discard,
}

/// Everything the ingest/refresh pipeline needs.
pub struct IngestCtx {
    pub(crate) pool: SqlitePool,
    pub(crate) dirs: Libraries,
    pub(crate) resolver: Resolver,
    pub(crate) grobid: Option<Grobid>,
}

impl IngestCtx {
    pub fn new(
        pool: SqlitePool,
        dirs: Libraries,
        resolver: Resolver,
        grobid: Option<Grobid>,
    ) -> Self {
        Self {
            pool,
            dirs,
            resolver,
            grobid,
        }
    }

    /// Build the full ingest stack from config. The retry policy is the
    /// caller's one decision: `production()` for CLI batch work,
    /// `interactive()` for serving (uploads answer synchronously).
    pub(crate) fn from_config(cfg: &Config, pool: SqlitePool, retry: RetryPolicy) -> Result<Self> {
        let resolver = Resolver::new_with_policy(cfg.contact_email.as_deref(), retry)?;
        let grobid = cfg.grobid_url.as_deref().map(Grobid::new).transpose()?;
        Ok(Self {
            pool,
            dirs: Libraries::under(&cfg.inbox_dir, &cfg.library_root),
            resolver,
            grobid,
        })
    }

    /// Ingest a single PDF with no identifier hint (text extraction decides),
    /// archiving the original out of the inbox.
    pub async fn ingest_file(&self, path: &Path) -> Result<Outcome> {
        self.ingest_file_with_hint(path, None, Disposal::Archive)
            .await
    }

    /// Stage `bytes` under a collision-safe name in the staging dir, ingest
    /// the staged file, and delete it on ingest failure. Shared by CLI import,
    /// web upload, and web URL import.
    pub async fn ingest_bytes(
        &self,
        bytes: &[u8],
        name: &str,
        hint: Option<Identifier>,
    ) -> Result<Outcome> {
        // Reduce `name` to its basename so a hostile filename cannot escape
        // the staging dir.
        let stem = Path::new(name)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("import.pdf");
        let staged = self
            .dirs
            .staging_dir
            .join(format!("{}-{stem}", Uuid::now_v7()));
        tokio::fs::create_dir_all(&self.dirs.staging_dir)
            .await
            .context("import staging dir")?;
        tokio::fs::write(&staged, bytes)
            .await
            .context("import stage write")?;
        match self
            .ingest_file_with_hint(&staged, hint, Disposal::Discard)
            .await
        {
            Ok(outcome) => Ok(outcome),
            Err(e) => {
                let _ = tokio::fs::remove_file(&staged).await;
                Err(e)
            }
        }
    }

    /// Ingest a single PDF, optionally seeding metadata resolution with a known
    /// identifier (used by URL/DOI import, where we already know the id and the
    /// PDF's first page may not print it). `disposal` decides what happens to
    /// the source once an outcome is reached; on `Err` it is left in place.
    pub async fn ingest_file_with_hint(
        &self,
        path: &Path,
        hint: Option<Identifier>,
        disposal: Disposal,
    ) -> Result<Outcome> {
        let outcome = self.ingest_inner(path, hint).await?;
        // The one disposal point, shared by every outcome.
        match disposal {
            Disposal::Archive => move_to_async(path, &self.dirs.processed_dir).await?,
            Disposal::Discard => tokio::fs::remove_file(path)
                .await
                .context("discard ingest source")?,
        }
        Ok(outcome)
    }

    async fn ingest_inner(&self, path: &Path, hint: Option<Identifier>) -> Result<Outcome> {
        let path = path.to_path_buf();

        // 1. Hash (blocking IO off the async runtime).
        let content_hash = {
            let p = path.clone();
            tokio::task::spawn_blocking(move || hash::sha256_file(&p)).await??
        };

        // 2. Dedup by content (active → Duplicate, trashed → InTrash).
        if let Some(existing) = papers::find_by_hash(&self.pool, &content_hash).await? {
            return Ok(Outcome::from_hash_hit(&existing));
        }

        // 3. Extract, identify, optionally GROBID, and resolve (factored for reuse).
        let ResolveInputs {
            ident,
            provisional_title,
            extracted,
            resolution,
        } = self.resolve_pdf(&path, hint).await?;

        // 4. Decide the stored fields, then the cite-key filename.
        let fields = resolve_fields(provisional_title, extracted, &ident, resolution);

        // 4b. A different file of a work we already have (same DOI/arXiv id)?
        if let Some(existing) = papers::find_by_identifier(
            &self.pool,
            fields.doi.as_deref(),
            fields.arxiv_id.as_deref(),
        )
        .await?
        {
            return Ok(Outcome::from_identifier_hit(&existing));
        }

        // 5. Pick a free cite key and file the PDF under it exclusively.
        // Disambiguation is read-compute-write with no lock, so two concurrent
        // ingests sharing a base key can compute the same "free" key; the
        // exclusive create turns that into a collision the loser observes.
        // The loser must not merely re-query the DB — the winner copies before
        // inserting, so its key may not be a row yet; the collided key joins a
        // local taken set instead.
        let base = naming::cite_key_base(&fields.authors.0, fields.year, fields.title.as_deref());
        let mut taken = match base.as_deref() {
            Some(b) => papers::cite_keys_with_base(&self.pool, b, None).await?,
            None => std::collections::HashSet::new(),
        };
        let (cite_key, rel_path, dest, created_excl) = loop {
            let cite_key = base.as_deref().map(|b| naming::disambiguate(b, &taken));
            let rel_path = naming::library_rel_path(cite_key.as_deref(), &content_hash);
            let dest = self.dirs.library_root.join(&rel_path);
            match cite_key {
                Some(key) => match copy_to_excl_async(&path, &dest).await {
                    Ok(()) => break (Some(key), rel_path, dest, true),
                    Err(e) if is_already_exists(&e) => {
                        taken.insert(key);
                    }
                    Err(e) => return Err(e),
                },
                // No cite key: the path is content-addressed
                // (`_unsorted/<hash>.pdf`), so an existing dest already holds
                // these bytes (or a truncated copy from a crashed run) — a
                // plain overwrite is a repair, never a clobber.
                None => {
                    copy_to_async(&path, &dest).await?;
                    break (None, rel_path, dest, false);
                }
            }
        };

        // 6. Build and store the record.
        let paper = fields.into_paper(content_hash, rel_path, cite_key);
        if let Err(e) = papers::insert(&self.pool, &paper).await {
            // Clean up only a file this ingest exclusively created: the
            // content-addressed path may be shared with — and still referenced
            // by — the concurrent winner of a same-bytes race.
            if created_excl {
                let _ = tokio::fs::remove_file(&dest).await;
            }
            // Lost a race with a concurrent ingest of the same work? Report the
            // winner's outcome instead of surfacing a constraint error.
            if db::is_unique_violation(&e) {
                tracing::warn!(
                    "insert hit a UNIQUE constraint (concurrent ingest of the same work?); re-checking"
                );
                if let Some(outcome) = recover_unique_collision(
                    &self.pool,
                    &paper.content_hash,
                    paper.meta.doi.as_deref(),
                    paper.meta.arxiv_id.as_deref(),
                )
                .await?
                {
                    return Ok(outcome);
                }
            }
            return Err(e);
        }

        Ok(Outcome::Ingested(paper.id))
    }

    /// Extract first-page text, identify a DOI/arXiv id, optionally enrich via GROBID
    /// (title-only path), and resolve authoritative metadata. Degrades to
    /// `None` on any resolver/network failure — never aborts.
    pub(crate) async fn resolve_pdf(
        &self,
        path: &Path,
        hint: Option<Identifier>,
    ) -> Result<ResolveInputs> {
        // Extract first-page text (blocking IO off the async runtime) and identify.
        let text = {
            let p = path.to_path_buf();
            tokio::task::spawn_blocking(move || pdf::extract_text(&p, 1)).await??
        };
        let ident = hint.unwrap_or_else(|| identify::identify(&text));
        let provisional_title = identify::guess_title(&text);

        // For the title-only path, optionally use GROBID for a better header
        // (degrades to None on failure).
        let extracted: Option<ResolvedMetadata> = match (&ident, self.grobid.as_ref()) {
            (Identifier::None, Some(g)) => match g.extract_header(path).await {
                Ok(md) => md,
                Err(e) => {
                    tracing::warn!("grobid extraction failed: {e}");
                    None
                }
            },
            _ => None,
        };

        // Search query prefers the GROBID title, else the heuristic first line.
        let title_hint: Option<String> = extracted
            .as_ref()
            .and_then(|m| m.title.clone())
            .or_else(|| provisional_title.clone());

        // The first-page text goes along as corroboration: a title alone cannot
        // tell this paper from a same-titled work by other authors.
        let query = title_hint
            .as_deref()
            .map(|title| TitleQuery { title, text: &text });
        let resolution = self.resolver.resolve(&ident, query).await;
        Ok(ResolveInputs {
            ident,
            provisional_title,
            extracted,
            resolution,
        })
    }

    /// Persist `paper` and re-file its PDF to the cite-key path implied by its
    /// current metadata. Copy-first ordering: copy the file, update the row,
    /// remove the old file — a failure at any step never leaves the DB
    /// pointing at a missing file. If the current file is missing or
    /// unreadable, the copy step fails and only the metadata is persisted —
    /// the row keeps its previous `rel_path`/`cite_key` and the caller gets
    /// `Ok(false)`. Returns whether the file moved.
    pub(crate) async fn save_and_refile(&self, paper: &mut Paper) -> Result<bool> {
        let pdf = self.dirs.library_root.join(&paper.rel_path);
        let cite_key = match naming::cite_key_base(
            &paper.meta.authors.0,
            paper.meta.year,
            paper.meta.title.as_deref(),
        ) {
            Some(base) => {
                let taken = papers::cite_keys_with_base(&self.pool, &base, Some(&paper.id)).await?;
                Some(naming::disambiguate(&base, &taken))
            }
            None => None,
        };
        let new_rel = naming::library_rel_path(cite_key.as_deref(), &paper.content_hash);
        let mut refiled_paths: Option<(std::path::PathBuf, std::path::PathBuf)> = None; // (old, new)
        if new_rel != paper.rel_path {
            let to = self.dirs.library_root.join(&new_rel);
            // Exclusive create: an occupied destination usually means a
            // concurrent filing race and lands in the same leave-in-place
            // degradation as any other copy failure. The exception is a file
            // holding this paper's own bytes — a crash between a previous
            // refile's copy and its row update leaves exactly that, and every
            // retry would hit it forever; adopt it and finish the refile.
            match copy_to_excl_async(&pdf, &to).await {
                Ok(()) => {
                    refiled_paths = Some((pdf.clone(), to));
                    paper.rel_path = new_rel;
                    paper.cite_key = cite_key;
                }
                Err(e) => {
                    if is_already_exists(&e) && file_hash_matches(&to, &paper.content_hash).await {
                        refiled_paths = Some((pdf.clone(), to));
                        paper.rel_path = new_rel;
                        paper.cite_key = cite_key;
                    } else {
                        tracing::warn!(
                            "re-file copy failed for {}: {e}; leaving in place",
                            paper.id
                        )
                    }
                }
            }
        }

        if let Err(e) = papers::update(&self.pool, paper).await {
            // Roll the copy back so filesystem and DB stay consistent.
            if let Some((_, new_path)) = &refiled_paths {
                let _ = tokio::fs::remove_file(new_path).await;
            }
            return Err(e);
        }
        if let Some((old_path, _)) = &refiled_paths {
            if let Err(e) = tokio::fs::remove_file(old_path).await {
                tracing::warn!("could not remove old file {}: {e}", old_path.display());
            }
        }
        Ok(refiled_paths.is_some())
    }

    /// Apply user-confirmed metadata to `paper`: guard identifier conflicts,
    /// overwrite the metadata block (keeping the old abstract when the source
    /// has none), mark it resolved, then persist + re-file.
    pub(crate) async fn apply_match(
        &self,
        paper: &mut Paper,
        md: ResolvedMetadata,
    ) -> Result<IdentifyOutcome> {
        if paper.deleted_at.is_some() {
            return Ok(IdentifyOutcome::Trashed);
        }
        if let Some(existing) =
            papers::find_by_identifier(&self.pool, md.doi.as_deref(), md.arxiv_id.as_deref())
                .await?
        {
            if existing.id != paper.id {
                return Ok(IdentifyOutcome::SameWork(existing.id));
            }
        }
        let mut meta = paper_meta(md);
        meta.abstract_text = meta
            .abstract_text
            .or_else(|| paper.meta.abstract_text.clone());
        paper.meta = meta;
        // No file-existence pre-check: fixing metadata must succeed even if the
        // PDF is missing; save_and_refile degrades to metadata-only in that case.
        self.save_and_refile(paper).await?;
        Ok(IdentifyOutcome::Applied)
    }
}

/// Result of applying a user-confirmed identify match.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum IdentifyOutcome {
    Applied,
    /// The chosen identifier already belongs to this other paper; no changes.
    SameWork(String),
    /// The paper is in the trash; restore it first. No changes.
    Trashed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::library::model::{Authors, Paper, PaperMeta, PaperStatus};
    use crate::library::{papers, trash};

    fn paper(id: &str, hash: &str, doi: Option<&str>) -> Paper {
        Paper {
            id: id.into(),
            content_hash: hash.into(),
            rel_path: format!("{hash}.pdf"),
            cite_key: None,
            added_at: "2026-07-08T00:00:00Z".into(),
            deleted_at: None,
            starred: false,
            name: None,
            meta: PaperMeta {
                title: Some("T".into()),
                abstract_text: None,
                authors: Authors::default(),
                venue: None,
                year: None,
                doi: doi.map(str::to_string),
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: PaperStatus::NeedsReview,
            },
        }
    }

    #[tokio::test]
    async fn ingest_bytes_sanitizes_the_name_and_cleans_up_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let pool = db::connect(&url).await.unwrap();
        let inbox = dir.path().join("inbox");
        let ctx = IngestCtx::new(
            pool,
            Libraries::under(&inbox, &dir.path().join("library")),
            Resolver::with_bases(
                None,
                "http://127.0.0.1:1".to_string(),
                "http://127.0.0.1:1".to_string(),
            )
            .unwrap(),
            None,
        );

        // Junk bytes fail extraction. The hostile name must stage as a bare
        // basename inside _uploads, and the staged copy must not survive the
        // failed ingest.
        let out = ctx.ingest_bytes(b"not a pdf", "../evil.pdf", None).await;
        assert!(out.is_err());
        assert!(!dir.path().join("evil.pdf").exists());
        assert!(!inbox.join("evil.pdf").exists());
        assert_eq!(
            std::fs::read_dir(&ctx.dirs.staging_dir).unwrap().count(),
            0,
            "staged file must be removed on ingest failure"
        );
    }

    /// An `IngestCtx` wired to dead resolver endpoints (no network use).
    async fn offline_ctx(root: &Path, library: &Path) -> IngestCtx {
        let url = format!("sqlite:{}", root.join("t.db").display());
        let pool = db::connect(&url).await.unwrap();
        IngestCtx::new(
            pool,
            Libraries::under(root, library),
            Resolver::with_bases(
                None,
                "http://127.0.0.1:1".to_string(),
                "http://127.0.0.1:1".to_string(),
            )
            .unwrap(),
            None,
        )
    }

    /// Seed a paper whose metadata refiles to `he2016deep.pdf`, with `bytes`
    /// at `old.pdf`.
    async fn refile_fixture(ctx: &IngestCtx, library: &Path, bytes: &[u8]) -> Paper {
        std::fs::create_dir_all(library).unwrap();
        let src = library.join("hash-me.tmp");
        std::fs::write(&src, bytes).unwrap();
        let content_hash = crate::hash::sha256_file(&src).unwrap();
        std::fs::remove_file(&src).unwrap();
        let mut p = paper("01890000-0000-7000-8000-0000000000c3", &content_hash, None);
        p.rel_path = "old.pdf".into();
        p.meta.title = Some("Deep Residual Learning".into());
        p.meta.authors = Authors(vec!["Kaiming He".into()]);
        p.meta.year = Some(2016);
        papers::insert(&ctx.pool, &p).await.unwrap();
        std::fs::write(library.join("old.pdf"), bytes).unwrap();
        p
    }

    #[tokio::test]
    async fn save_and_refile_adopts_its_own_completed_copy() {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("library");
        let ctx = offline_ctx(dir.path(), &library).await;
        let bytes = b"%PDF-1.4 same bytes";
        let mut p = refile_fixture(&ctx, &library, bytes).await;
        // A previous refile's completed copy, orphaned by a crash before the
        // row update: the destination already holds this paper's own bytes.
        std::fs::write(library.join("he2016deep.pdf"), bytes).unwrap();

        assert!(ctx.save_and_refile(&mut p).await.unwrap());
        assert_eq!(p.rel_path, "he2016deep.pdf");
        assert_eq!(p.cite_key.as_deref(), Some("he2016deep"));
        assert!(!library.join("old.pdf").exists(), "old file removed");
        assert_eq!(
            std::fs::read(library.join("he2016deep.pdf")).unwrap(),
            bytes
        );
        let row = papers::get(&ctx.pool, &p.id).await.unwrap().unwrap();
        assert_eq!(row.rel_path, "he2016deep.pdf");
    }

    #[tokio::test]
    async fn save_and_refile_still_defers_to_a_foreign_occupant() {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("library");
        let ctx = offline_ctx(dir.path(), &library).await;
        let mut p = refile_fixture(&ctx, &library, b"%PDF-1.4 same bytes").await;
        // Same key, different work: a concurrent winner's file must not be
        // adopted.
        std::fs::write(library.join("he2016deep.pdf"), b"other work").unwrap();

        assert!(!ctx.save_and_refile(&mut p).await.unwrap());
        assert_eq!(p.rel_path, "old.pdf", "left in place");
        assert!(library.join("old.pdf").exists());
        assert_eq!(
            std::fs::read(library.join("he2016deep.pdf")).unwrap(),
            b"other work",
            "occupant untouched"
        );
    }

    #[tokio::test]
    async fn apply_match_updates_conflicts_and_keeps_abstract() {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let pool = db::connect(&url).await.unwrap();
        let library = dir.path().join("library");
        std::fs::create_dir_all(&library).unwrap();
        let ctx = IngestCtx::new(
            pool.clone(),
            Libraries::under(dir.path(), &library),
            Resolver::with_bases(
                None,
                "http://127.0.0.1:1".to_string(),
                "http://127.0.0.1:1".to_string(),
            )
            .unwrap(),
            None,
        );

        // Seed a needs_review paper with a GROBID abstract and a real file.
        let mut a = paper("01890000-0000-7000-8000-0000000000a1", "ha", None);
        a.meta.abstract_text = Some("kept abstract".into());
        std::fs::write(library.join("ha.pdf"), b"%PDF-1.4 fake").unwrap();
        papers::insert(&pool, &a).await.unwrap();
        // A manual name must survive identify: apply_match replaces the whole
        // metadata block, and `name` lives outside it precisely for this.
        let antifuzz = papers::PaperName::parse("AntiFuzz").unwrap();
        papers::set_name(&pool, &a.id, antifuzz.as_ref())
            .await
            .unwrap();

        // Another paper already owns a DOI (for the conflict case).
        let b = paper(
            "01890000-0000-7000-8000-0000000000b2",
            "hb",
            Some("10.9/owned"),
        );
        papers::insert(&pool, &b).await.unwrap();

        // Conflict: applying b's DOI to a -> SameWork(b.id), nothing changed.
        let md_conflict = ResolvedMetadata {
            title: Some("X".into()),
            doi: Some("10.9/owned".into()),
            source: "crossref".into(),
            ..Default::default()
        };
        let out = ctx.apply_match(&mut a.clone(), md_conflict).await.unwrap();
        assert_eq!(out, IdentifyOutcome::SameWork(b.id.clone()));
        let unchanged = papers::get(&pool, &a.id).await.unwrap().unwrap();
        assert_eq!(unchanged.meta.status, PaperStatus::NeedsReview);

        // Apply: DBLP-style metadata without an abstract keeps the old abstract,
        // sets Resolved, recomputes cite key and re-files.
        let md = ResolvedMetadata {
            title: Some("AntiFuzz: Impeding Fuzzing Audits of Binary Executables".into()),
            authors: vec!["Emre Güler".into(), "Thorsten Holz".into()],
            venue: Some("USENIX Security Symposium".into()),
            year: Some(2019),
            dblp_key: Some("conf/uss/GulerAAH19".into()),
            source: "dblp".into(),
            ..Default::default()
        };
        let out = ctx.apply_match(&mut a, md.clone()).await.unwrap();
        assert_eq!(out, IdentifyOutcome::Applied);
        let got = papers::get(&pool, &a.id).await.unwrap().unwrap();
        assert_eq!(got.meta.status, PaperStatus::Resolved);
        assert_eq!(got.meta.abstract_text.as_deref(), Some("kept abstract"));
        assert_eq!(got.cite_key.as_deref(), Some("guler2019antifuzz"));
        assert_eq!(got.rel_path, "guler2019antifuzz.pdf");
        assert_eq!(got.name.as_deref(), Some("AntiFuzz"));
        assert!(library.join("guler2019antifuzz.pdf").exists());

        // Idempotent: re-applying the same match succeeds and changes nothing.
        let out = ctx.apply_match(&mut a, md.clone()).await.unwrap();
        assert_eq!(out, IdentifyOutcome::Applied);
        let again = papers::get(&pool, &a.id).await.unwrap().unwrap();
        assert_eq!(again.cite_key.as_deref(), Some("guler2019antifuzz"));
        assert_eq!(again.rel_path, "guler2019antifuzz.pdf");
        assert!(library.join("guler2019antifuzz.pdf").exists());

        // Trashed: a soft-deleted paper is guarded inside apply_match itself,
        // not just at the CLI/web call sites.
        trash::soft_delete(&pool, &a.id).await.unwrap();
        a.deleted_at = Some("x".into());
        let out = ctx.apply_match(&mut a, md).await.unwrap();
        assert_eq!(out, IdentifyOutcome::Trashed);
        let still_trashed = papers::get(&pool, &a.id).await.unwrap().unwrap();
        assert_eq!(still_trashed.cite_key.as_deref(), Some("guler2019antifuzz"));
        assert_eq!(still_trashed.rel_path, "guler2019antifuzz.pdf");
    }
}
