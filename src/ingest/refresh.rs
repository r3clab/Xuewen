use anyhow::Result;

use crate::ingest::pipeline::{IngestCtx, Saved};
use crate::library::model::{Identifier, Paper, PaperStatus};
use crate::library::papers;

/// Which papers a refresh pass re-resolves. Every processed paper is re-filed
/// regardless; this only controls whose metadata is re-fetched.
pub enum RefreshTarget {
    /// Default: re-resolve `needs_review` records; re-file all papers.
    NeedsReview,
    /// Re-resolve (and re-file) every paper.
    All,
    /// Re-resolve (and re-file) the single paper with this id (exact or unique prefix).
    One(String),
}

/// Tally of a refresh pass, for CLI feedback.
#[derive(Debug, Default)]
pub struct RefreshSummary {
    pub processed: usize,
    pub reresolved: usize,
    pub refiled: usize,
}

/// Run one refresh pass over the library. Each paper is handled independently:
/// a per-paper failure is logged and never aborts the run.
pub async fn run(ctx: &IngestCtx, target: RefreshTarget) -> Result<RefreshSummary> {
    let (papers, reresolve_all) = match target {
        RefreshTarget::NeedsReview => (papers::all(&ctx.pool).await?, false),
        RefreshTarget::All => (papers::all(&ctx.pool).await?, true),
        RefreshTarget::One(id) => {
            let p = papers::find_one(&ctx.pool, &id).await?;
            if p.deleted_at.is_some() {
                tracing::warn!("{} is in the trash; skipping refresh", p.id);
                return Ok(RefreshSummary::default());
            }
            (vec![p], true)
        }
    };

    let mut summary = RefreshSummary::default();
    for paper in papers {
        summary.processed += 1;
        let reresolve = reresolve_all || paper.meta.status == PaperStatus::NeedsReview;
        match refresh_one(ctx, &paper, reresolve).await {
            Ok(outcome) => {
                summary.reresolved += outcome.reresolved as usize;
                summary.refiled += outcome.refiled as usize;
            }
            Err(e) => tracing::warn!("refresh failed for {}: {e}", paper.id),
        }
    }
    Ok(summary)
}

#[derive(Default)]
struct OneOutcome {
    reresolved: bool,
    refiled: bool,
}

/// Refresh `snapshot`, the paper as the pass listed it. The network work runs
/// against the snapshot; the write applies to the row as it is right after,
/// so an edit made meanwhile survives — a paper trashed since is skipped, and
/// one whose metadata changed since (identified, or refreshed by another
/// pass) keeps that metadata.
async fn refresh_one(ctx: &IngestCtx, snapshot: &Paper, reresolve: bool) -> Result<OneOutcome> {
    let mut outcome = OneOutcome::default();
    let pdf = ctx.dirs.library_root.join(&snapshot.rel_path);
    if !tokio::fs::try_exists(&pdf).await.unwrap_or(false) {
        tracing::warn!(
            "library file missing for {} ({}); skipping",
            snapshot.id,
            snapshot.rel_path
        );
        return Ok(outcome);
    }

    // Re-resolve metadata from the stored PDF (best-effort; keep old data on failure).
    let fields = if reresolve {
        match ctx.resolve_pdf(&pdf, stored_identifier(snapshot)).await {
            Ok(inputs) => Some(inputs.into_meta()),
            Err(e) => {
                tracing::warn!(
                    "re-resolve failed for {}: {e}; keeping existing metadata",
                    snapshot.id
                );
                None
            }
        }
    } else {
        None
    };

    let Some(mut paper) = papers::get(&ctx.pool, &snapshot.id)
        .await?
        .filter(|p| p.deleted_at.is_none())
    else {
        tracing::info!("{} was trashed during refresh; skipping", snapshot.id);
        return Ok(outcome);
    };
    if let Some(fields) = fields {
        if paper.meta != snapshot.meta {
            tracing::info!(
                "{} was edited during refresh; keeping that metadata",
                paper.id
            );
        } else if fields.status == PaperStatus::NeedsReview
            && paper.meta.status == PaperStatus::Resolved
        {
            // Never downgrade an already-resolved record.
            tracing::warn!(
                "re-resolve of {} came back unresolved; keeping existing resolved metadata",
                paper.id
            );
        } else {
            paper.meta = fields;
            outcome.reresolved = true;
        }
    }

    match ctx.save_and_refile(&mut paper).await? {
        Saved::Refiled => outcome.refiled = true,
        Saved::InPlace => {}
        Saved::Trashed => outcome.reresolved = false,
    }
    Ok(outcome)
}

/// The paper's stored identifier: re-resolution looks it up before anything
/// the PDF text suggests, and keeps it whatever else resolves.
fn stored_identifier(paper: &Paper) -> Option<Identifier> {
    let meta = &paper.meta;
    meta.doi
        .clone()
        .map(Identifier::Doi)
        .or_else(|| meta.arxiv_id.clone().map(Identifier::Arxiv))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::pipeline::Libraries;
    use crate::library::trash;
    use crate::metadata::resolve::Resolver;
    use crate::testutil::{sample_paper, temp_pool, write_pdf};

    /// A context over `dir` holding one needs_review paper whose PDF re-resolves
    /// (offline) to a different provisional title than the stored one.
    async fn ctx_with_paper(dir: &std::path::Path) -> (tempfile::TempDir, IngestCtx, Paper) {
        let library = dir.join("library");
        std::fs::create_dir_all(&library).unwrap();
        write_pdf(&library.join("p.pdf"), &["Deep Residual Learning"]);
        let (db_dir, pool) = temp_pool().await;
        let mut p = sample_paper("01890000-0000-7000-8000-0000000000a1", "h");
        p.rel_path = "p.pdf".into();
        papers::insert(&pool, &p).await.unwrap();
        let ctx = IngestCtx::new(
            pool,
            Libraries::under(dir, &library),
            Resolver::offline(),
            None,
        );
        (db_dir, ctx, p)
    }

    #[tokio::test]
    async fn a_paper_trashed_during_refresh_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let (_db, ctx, snapshot) = ctx_with_paper(dir.path()).await;
        trash::soft_delete(&ctx.pool, &snapshot.id).await.unwrap();

        let out = refresh_one(&ctx, &snapshot, true).await.unwrap();
        assert!(!out.reresolved && !out.refiled);
        let row = papers::get(&ctx.pool, &snapshot.id).await.unwrap().unwrap();
        assert!(row.deleted_at.is_some(), "still in the trash");
        assert_eq!(row.meta, snapshot.meta);
        assert!(dir.path().join("library/p.pdf").exists());
    }

    #[tokio::test]
    async fn an_edit_made_during_refresh_survives_it() {
        let dir = tempfile::tempdir().unwrap();
        let (_db, ctx, snapshot) = ctx_with_paper(dir.path()).await;
        let mut edited = snapshot.clone();
        edited.meta.title = Some("Edited Meanwhile".into());
        assert!(papers::update(&ctx.pool, &edited).await.unwrap());

        let out = refresh_one(&ctx, &snapshot, true).await.unwrap();
        assert!(!out.reresolved);
        let row = papers::get(&ctx.pool, &snapshot.id).await.unwrap().unwrap();
        assert_eq!(row.meta.title.as_deref(), Some("Edited Meanwhile"));
    }

    #[tokio::test]
    async fn an_unedited_paper_takes_the_reresolved_fields() {
        let dir = tempfile::tempdir().unwrap();
        let (_db, ctx, snapshot) = ctx_with_paper(dir.path()).await;

        let out = refresh_one(&ctx, &snapshot, true).await.unwrap();
        assert!(out.reresolved);
        let row = papers::get(&ctx.pool, &snapshot.id).await.unwrap().unwrap();
        assert_eq!(row.meta.title.as_deref(), Some("Deep Residual Learning"));
    }
}
