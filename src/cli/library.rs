use anyhow::Result;
use std::path::PathBuf;

use super::output::confirm;
use super::Ctx;
use crate::library::{papers, projects, trash};

#[derive(clap::Args)]
pub(super) struct DeleteArgs {
    /// Paper id (exact or unique prefix).
    id: String,
    /// Skip the confirmation prompt.
    #[arg(long)]
    yes: bool,
}

#[derive(clap::Args)]
pub(super) struct RestoreArgs {
    /// Paper id (exact or unique prefix).
    id: String,
}

#[derive(clap::Args)]
pub(super) struct PurgeArgs {
    /// A trashed paper id (exact or unique prefix) to purge.
    #[arg(conflicts_with = "all")]
    id: Option<String>,
    /// Purge every trashed paper.
    #[arg(long)]
    all: bool,
    /// Skip the confirmation prompt.
    #[arg(long)]
    yes: bool,
}

#[derive(clap::Args)]
pub(super) struct StarArgs {
    paper: String,
}

#[derive(clap::Args)]
pub(super) struct ExportArgs {
    /// Paper id (exact or unique prefix) for a single entry.
    #[arg(conflicts_with_all = ["all", "project", "tag", "starred"])]
    id: Option<String>,
    /// Export the whole (non-trashed) library.
    #[arg(long, conflicts_with = "project")]
    all: bool,
    /// Export all papers in this project (name or id).
    #[arg(long)]
    project: Option<String>,
    /// Filter batch exports to papers carrying this tag (or its prefix).
    #[arg(long)]
    tag: Option<String>,
    /// Filter batch exports to starred papers only.
    #[arg(long)]
    starred: bool,
    /// Filter batch exports by a search term (title/author).
    #[arg(long)]
    query: Option<String>,
    /// Filter batch exports by status (resolved|needs_review).
    #[arg(long)]
    status: Option<String>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = BibFormatArg::Bibtex)]
    format: BibFormatArg,
    /// Write to this file instead of stdout.
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum BibFormatArg {
    Bibtex,
    Biblatex,
}

impl From<BibFormatArg> for crate::library::export::BibFormat {
    fn from(a: BibFormatArg) -> Self {
        match a {
            BibFormatArg::Bibtex => crate::library::export::BibFormat::Bibtex,
            BibFormatArg::Biblatex => crate::library::export::BibFormat::Biblatex,
        }
    }
}

pub(super) async fn delete(ctx: &Ctx, args: DeleteArgs) -> Result<()> {
    let paper = papers::find_one(&ctx.pool, &args.id).await?;
    if paper.deleted_at.is_some() {
        println!("already deleted: {}", paper.id);
    } else {
        let title = paper.meta.title.as_deref().unwrap_or("(untitled)");
        if args.yes || confirm(&format!("Delete {title:?}?"))? {
            trash::soft_delete(&ctx.pool, &paper.id).await?;
            println!("deleted {}", paper.id);
        } else {
            println!("cancelled");
        }
    }
    Ok(())
}

pub(super) async fn restore(ctx: &Ctx, args: RestoreArgs) -> Result<()> {
    let paper = papers::find_one(&ctx.pool, &args.id).await?;
    // Unlike delete's soft "already deleted" no-op, a restore of an active
    // paper is a hard error: it usually means a mistyped id prefix, and
    // silently "succeeding" would hide that.
    if paper.deleted_at.is_none() {
        anyhow::bail!("{} is not in the trash", paper.id);
    }
    trash::restore(&ctx.pool, &paper.id).await?;
    println!("restored {}", paper.id);
    Ok(())
}

pub(super) async fn purge(ctx: &Ctx, args: PurgeArgs) -> Result<()> {
    let targets = match (args.id, args.all) {
        (Some(id), _) => {
            let p = papers::find_one(&ctx.pool, &id).await?;
            if p.deleted_at.is_none() {
                anyhow::bail!("{} is not in the trash (delete it first)", p.id);
            }
            vec![p]
        }
        (None, true) => trash::list(&ctx.pool).await?,
        (None, false) => anyhow::bail!("specify an <ID> or --all"),
    };
    if targets.is_empty() {
        println!("trash is empty");
    } else if args.yes
        || confirm(&format!(
            "Permanently delete {} paper(s) and their files?",
            targets.len()
        ))?
    {
        // One paper's failure must not strand the rest half-purged:
        // keep going and report the stragglers at the end.
        let mut failures = 0usize;
        let app = ctx.app();
        for p in &targets {
            if let Err(e) = app.purge(p).await {
                eprintln!("could not purge {}: {e:#}", p.id);
                failures += 1;
            }
        }
        println!("purged {} paper(s)", targets.len() - failures);
        if failures > 0 {
            anyhow::bail!("{failures} paper(s) could not be purged — re-run to retry");
        }
    } else {
        println!("cancelled");
    }
    Ok(())
}

pub(super) async fn star(ctx: &Ctx, args: StarArgs, on: bool) -> Result<()> {
    let paper = papers::find_one(&ctx.pool, &args.paper).await?;
    let label = paper.cite_key.as_deref().unwrap_or(&paper.id);
    if papers::set_starred(&ctx.pool, &paper.id, on).await? {
        println!("{} {label}", if on { "starred" } else { "unstarred" });
    } else {
        println!("{label} not found");
    }
    Ok(())
}

pub(super) async fn export(ctx: &Ctx, args: ExportArgs) -> Result<()> {
    let ExportArgs {
        id,
        all,
        project,
        tag,
        starred,
        query,
        status,
        format,
        output,
    } = args;
    let fmt = crate::library::export::BibFormat::from(format);
    let text = if let Some(id) = id {
        let paper = papers::find_one(&ctx.pool, &id).await?;
        crate::library::export::format_entry(&paper, fmt)
    } else {
        if !all && project.is_none() && tag.is_none() && !starred {
            anyhow::bail!(
                "specify a paper id, --all, --project <name>, --tag <name>, or --starred"
            );
        }
        let project_id = match &project {
            Some(sel) => Some(projects::find_one(&ctx.pool, sel).await?.id),
            None => None,
        };
        let filter = papers::PaperFilter {
            status,
            project: project_id,
            tag,
            starred: starred.then_some(true),
        };
        let papers = papers::list(&ctx.pool, query.as_deref(), None, &filter).await?;
        crate::library::export::format_entries(&papers, fmt)
    };
    // Normalize to exactly one trailing newline so single-entry output
    // (which has none) doesn't abut the shell prompt; batch output is
    // already newline-terminated and is left unchanged.
    let text = if text.ends_with('\n') {
        text
    } else {
        format!("{text}\n")
    };
    match output {
        Some(path) => {
            tokio::fs::write(&path, &text).await?;
            println!("wrote {}", path.display());
        }
        None => print!("{text}"),
    }
    Ok(())
}
