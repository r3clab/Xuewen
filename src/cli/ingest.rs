use anyhow::Result;
use std::path::PathBuf;

use super::output::print_outcome;
use super::serve::cancel_on_signal;
use super::Ctx;
use crate::ingest::import::{ImportError, Imported};
use crate::ingest::refresh::RefreshTarget;
use crate::search::vector::{OpenError, OpenMode};
use crate::search::{indexer, SearchService, Semantic};

#[derive(clap::Args)]
pub(super) struct IngestArgs {
    path: PathBuf,
}

#[derive(clap::Args)]
pub(super) struct ImportArgs {
    input: String,
}

#[derive(clap::Args)]
pub(super) struct RefreshArgs {
    /// Paper id (exact or unique prefix) to refresh. Omit to refresh needs_review records.
    #[arg(conflicts_with = "all")]
    id: Option<String>,
    /// Re-resolve every paper, not just needs_review records.
    #[arg(long)]
    all: bool,
}

pub(super) async fn ingest(ctx: &Ctx, args: IngestArgs) -> Result<()> {
    let outcome = ctx.app().ingest_path(&args.path).await?;
    print_outcome(&ctx.pool, outcome).await
}

pub(super) async fn import(ctx: &Ctx, args: ImportArgs) -> Result<()> {
    let input = args.input;
    match ctx.app().import(&input).await {
        Ok(Imported::Ingested(outcome)) => print_outcome(&ctx.pool, outcome).await,
        Ok(Imported::Unfetched { title, .. }) => {
            let title = title.as_deref().unwrap_or("(unknown title)");
            anyhow::bail!(
                "could not fetch a PDF for {title:?} — paywalled with no open-access \
                 copy, or the cookie is missing/expired. Download it in your browser \
                 and drop it in the inbox."
            )
        }
        Err(ImportError::Unsupported) => {
            anyhow::bail!("could not recognize {input:?} as a URL, DOI, or arXiv id")
        }
        Err(ImportError::CookieExpired) => anyhow::bail!(
            "proxy session expired — refresh it: xuewen proxy-cookie --set '<cookie>'"
        ),
        Err(ImportError::Network(e)) => Err(e.context("fetch failed")),
        Err(ImportError::Ingest(e)) => Err(e),
        Err(ImportError::Unavailable(m)) => Err(anyhow::Error::msg(m)),
        Err(ImportError::Internal(e)) => Err(e),
    }
}

pub(super) async fn watch(ctx: &Ctx) -> Result<()> {
    let app = ctx.app();
    let semantic = Semantic::open(
        &ctx.pool,
        &ctx.cfg.search,
        &ctx.cfg.ai,
        OpenMode::LoadOrCreate,
    )
    .await?;
    let vectors = semantic.store().cloned();
    match semantic {
        // The holder runs its own sweeps; a second Tantivy writer here would
        // only fail every paper into backoff.
        Semantic::Unavailable(e @ OpenError::Locked { .. }) => {
            tracing::warn!("search indexing disabled: {e}")
        }
        semantic => match SearchService::open(ctx.pool.clone(), &ctx.cfg.search, semantic).await {
            Ok(s) => indexer::spawn(app.tasks(), s, ctx.cfg.library_root.clone()),
            Err(e) => tracing::warn!("search indexing disabled: {e}"),
        },
    }
    tokio::spawn(cancel_on_signal(app.tasks().token()));
    let watched = app.watch(&ctx.cfg.inbox_dir).await;
    app.shutdown().await;
    if let Some(vectors) = vectors {
        vectors.close_gracefully().await;
    }
    Ok(watched?)
}

pub(super) async fn refresh(ctx: &Ctx, args: RefreshArgs) -> Result<()> {
    let target = match (args.id, args.all) {
        (Some(id), _) => RefreshTarget::One(id),
        (None, true) => RefreshTarget::All,
        (None, false) => RefreshTarget::NeedsReview,
    };
    let summary = ctx.app().refresh(target).await?;
    println!(
        "refresh: {} processed, {} re-resolved, {} re-filed",
        summary.processed, summary.reresolved, summary.refiled
    );
    Ok(())
}
