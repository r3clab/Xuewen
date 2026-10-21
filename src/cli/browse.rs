use anyhow::Result;
use clap::builder::PossibleValuesParser;
use sqlx::SqlitePool;

use super::{output, Ctx};
use crate::library::model::{Paper, PaperStatus};
use crate::library::{papers, projects, tags};

/// The library filters shared by `list` and `export`.
#[derive(clap::Args)]
pub(super) struct FilterArgs {
    /// Only papers in this project (name or id).
    #[arg(long)]
    pub(super) project: Option<String>,
    /// Only papers carrying this tag (or a tag nested under it).
    #[arg(long)]
    pub(super) tag: Option<String>,
    /// Only starred papers.
    #[arg(long)]
    pub(super) starred: bool,
    /// Only papers whose title or authors contain this term.
    #[arg(long)]
    pub(super) query: Option<String>,
    /// Only papers with this status.
    #[arg(long, value_parser = PossibleValuesParser::new(papers::STATUS_KEYS))]
    pub(super) status: Option<String>,
}

impl FilterArgs {
    pub(super) async fn resolve(&self, pool: &SqlitePool) -> Result<papers::PaperFilter> {
        let project = match &self.project {
            Some(sel) => Some(projects::find_one(pool, sel).await?.id),
            None => None,
        };
        Ok(papers::PaperFilter {
            status: self.status.clone(),
            project,
            tag: self.tag.clone(),
            starred: self.starred.then_some(true),
        })
    }
}

#[derive(clap::Args)]
pub(super) struct ListArgs {
    #[command(flatten)]
    filters: FilterArgs,
    /// Sort order.
    #[arg(long, value_parser = PossibleValuesParser::new(papers::SORT_KEYS))]
    sort: Option<String>,
}

#[derive(clap::Args)]
pub(super) struct ShowArgs {
    /// Paper id (exact or unique prefix).
    paper: String,
}

pub(super) async fn list(ctx: &Ctx, args: ListArgs) -> Result<()> {
    let filter = args.filters.resolve(&ctx.pool).await?;
    let papers = papers::list(
        &ctx.pool,
        args.filters.query.as_deref(),
        args.sort.as_deref(),
        &filter,
    )
    .await?;
    if papers.is_empty() {
        println!("no papers");
    }
    for p in &papers {
        println!(
            "{}  {}  {}{}  {}",
            p.id,
            p.meta.year.map_or("----".to_string(), |y| y.to_string()),
            if p.starred { "*" } else { " " },
            p.cite_key.as_deref().unwrap_or("-"),
            output::title(p.meta.title.as_deref())
        );
    }
    Ok(())
}

pub(super) async fn show(ctx: &Ctx, args: ShowArgs) -> Result<()> {
    let pool = &ctx.pool;
    let p = papers::find_one(pool, &args.paper).await?;
    let tags: Vec<String> = tags::for_paper(pool, &p.id)
        .await?
        .into_iter()
        .map(|t| t.name)
        .collect();
    let projects: Vec<String> = projects::for_paper(pool, &p.id)
        .await?
        .into_iter()
        .map(|p| p.name)
        .collect();
    let code = crate::agent::store::get_paper_code(pool, &p.id)
        .await?
        .map(|c| {
            let at = c.commit_sha.map(|s| format!(" @ {s}")).unwrap_or_default();
            format!("{} ({}{at})", c.repo_url, c.status)
        });
    let summary = crate::summary::store::get(pool, &p.id).await?;

    println!("{}", output::title(p.meta.title.as_deref()));
    let refs = [
        ("tags:", Some(tags.join(", ")).filter(|t| !t.is_empty())),
        (
            "projects:",
            Some(projects.join(", ")).filter(|p| !p.is_empty()),
        ),
        ("code:", code),
    ];
    for (label, value) in details(&p).into_iter().chain(refs) {
        if let Some(value) = value {
            println!("  {label:<9} {value}");
        }
    }
    if let Some(abstract_text) = &p.meta.abstract_text {
        println!("\nAbstract\n  {abstract_text}");
    }
    if let Some(s) = summary {
        println!("\nSummary");
        for (label, text) in [
            ("TL;DR", &s.tldr),
            ("Problem", &s.problem),
            ("Approach", &s.approach),
            ("Results", &s.results),
            ("Limits", &s.limitations),
        ] {
            println!("  {label}: {text}");
        }
    }
    Ok(())
}

fn details(p: &Paper) -> Vec<(&'static str, Option<String>)> {
    let venue = match (&p.meta.venue, p.meta.year) {
        (Some(v), Some(y)) => Some(format!("{v} {y}")),
        (Some(v), None) => Some(v.clone()),
        (None, Some(y)) => Some(y.to_string()),
        (None, None) => None,
    };
    let status = match p.meta.status {
        PaperStatus::Resolved => "resolved",
        PaperStatus::NeedsReview => "needs review",
    };
    vec![
        ("id:", Some(p.id.clone())),
        ("cite key:", p.cite_key.clone()),
        ("name:", p.name.clone()),
        (
            "authors:",
            Some(p.meta.authors.0.join(", ")).filter(|a| !a.is_empty()),
        ),
        ("venue:", venue),
        ("doi:", p.meta.doi.clone()),
        ("arxiv:", p.meta.arxiv_id.clone()),
        ("url:", p.meta.url.clone()),
        ("status:", Some(status.to_string())),
        ("added:", Some(p.added_at.clone())),
        ("starred:", p.starred.then(|| "yes".to_string())),
        (
            "trashed:",
            p.deleted_at
                .as_ref()
                .map(|at| format!("{at} — run: xuewen restore {}", p.id)),
        ),
    ]
}

pub(super) async fn stats(ctx: &Ctx) -> Result<()> {
    let (total, resolved, needs_review) = papers::stats(&ctx.pool).await?;
    println!("{total} papers ({resolved} resolved, {needs_review} need review)");
    Ok(())
}
