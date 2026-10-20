use anyhow::Result;

use crate::ingest::fields::{resolve_fields, ResolveInputs};
use crate::ingest::pipeline::IngestCtx;
use crate::library::model::{Paper, PaperStatus};
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
    for mut paper in papers {
        summary.processed += 1;
        let reresolve = reresolve_all || paper.meta.status == PaperStatus::NeedsReview;
        match refresh_one(ctx, &mut paper, reresolve).await {
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

async fn refresh_one(ctx: &IngestCtx, paper: &mut Paper, reresolve: bool) -> Result<OneOutcome> {
    let mut outcome = OneOutcome::default();
    let library_root = &ctx.dirs.library_root;
    let pdf = library_root.join(&paper.rel_path);
    if !tokio::fs::try_exists(&pdf).await.unwrap_or(false) {
        tracing::warn!(
            "library file missing for {} ({}); skipping",
            paper.id,
            paper.rel_path
        );
        return Ok(outcome);
    }

    // Re-resolve metadata from the stored PDF (best-effort; keep old data on failure).
    if reresolve {
        match ctx.resolve_pdf(&pdf, None).await {
            Ok(inputs) => {
                let ResolveInputs {
                    ident,
                    provisional_title,
                    extracted,
                    resolution,
                } = inputs;
                let fields = resolve_fields(provisional_title, extracted, &ident, resolution);
                // Never downgrade an already-resolved record: if this re-resolution
                // came back unconfident (needs_review) but the paper is already
                // resolved, keep the existing metadata rather than wiping it.
                let would_downgrade = fields.status == PaperStatus::NeedsReview
                    && paper.meta.status == PaperStatus::Resolved;
                if would_downgrade {
                    tracing::warn!(
                        "re-resolve of {} came back unresolved; keeping existing resolved metadata",
                        paper.id
                    );
                } else {
                    paper.meta = fields;
                    outcome.reresolved = true;
                }
            }
            Err(e) => tracing::warn!(
                "re-resolve failed for {}: {e}; keeping existing metadata",
                paper.id
            ),
        }
    }

    outcome.refiled = ctx.save_and_refile(paper).await?;
    Ok(outcome)
}
