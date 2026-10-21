use anyhow::Result;
use clap::Subcommand;

use super::{output, Ctx};
use crate::library::papers;
use crate::library::tags::{self, TagName};

#[derive(Subcommand)]
pub(super) enum TagCmd {
    /// List tags with paper counts.
    List,
    /// Add a tag to a paper (created if new).
    Add { paper: String, name: String },
    /// Remove a tag from a paper.
    Remove { paper: String, name: String },
    /// Rename a tag.
    Rename { old: String, new: String },
    /// Delete a tag from all papers.
    Rm { name: String },
    /// List papers carrying a tag (or its prefix).
    Show { name: String },
}

pub(super) async fn tag(ctx: &Ctx, cmd: TagCmd) -> Result<()> {
    let pool = &ctx.pool;
    match cmd {
        TagCmd::List => {
            let tags = tags::list(pool).await?;
            if tags.is_empty() {
                println!("no tags");
            }
            for s in tags {
                println!("{}  ({} papers)", s.tag.name, s.paper_count);
            }
        }
        TagCmd::Add { paper, name } => {
            let paper = papers::find_one(pool, &paper).await?;
            let label = paper.cite_key.as_deref().unwrap_or(&paper.id);
            let tag = tags::add_paper(pool, &paper.id, &tag_name(&name)?).await?;
            println!("added tag {} to {label}", tag.name);
        }
        TagCmd::Remove { paper, name } => {
            let paper = papers::find_one(pool, &paper).await?;
            let label = paper.cite_key.as_deref().unwrap_or(&paper.id);
            let tag = tags::find_by_name(pool, &name)
                .await?
                .ok_or_else(|| anyhow::anyhow!("no tag named {name:?}"))?;
            if tags::remove_paper(pool, &paper.id, &tag.id).await? {
                println!("removed tag {} from {label}", tag.name);
            } else {
                println!("{label} did not carry tag {}", tag.name);
            }
        }
        TagCmd::Rename { old, new } => {
            let tag = tags::find_by_name(pool, &old)
                .await?
                .ok_or_else(|| anyhow::anyhow!("no tag named {old:?}"))?;
            let renamed = tags::rename(pool, &tag.id, &tag_name(&new)?)
                .await?
                .ok_or_else(|| anyhow::anyhow!("no tag named {old:?}"))?;
            println!("renamed tag {} to {}", old, renamed.name);
        }
        TagCmd::Rm { name } => {
            let tag = tags::find_by_name(pool, &name)
                .await?
                .ok_or_else(|| anyhow::anyhow!("no tag named {name:?}"))?;
            tags::delete(pool, &tag.id).await?;
            println!("deleted tag {}", tag.name);
        }
        TagCmd::Show { name } => {
            let filter = papers::PaperFilter {
                tag: Some(name.clone()),
                ..Default::default()
            };
            let papers = papers::list(pool, None, None, &filter).await?;
            println!("{name} — {} paper(s)", papers.len());
            for p in papers {
                println!("  {}  {}", p.id, output::title(p.meta.title.as_deref()));
            }
        }
    }
    Ok(())
}

fn tag_name(raw: &str) -> Result<TagName> {
    TagName::parse(raw).map_err(|_| anyhow::anyhow!("empty tag name"))
}
