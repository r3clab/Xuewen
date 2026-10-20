use anyhow::Result;
use clap::Subcommand;

use super::output::{confirm, ellipsize};
use super::Ctx;
use crate::library::papers;

#[derive(Subcommand)]
pub(super) enum AnnotationCmd {
    /// List a paper's annotations in reading order.
    List { paper: String },
    /// Delete one annotation by id.
    Rm { paper: String, id: String },
    /// Delete every annotation on a paper.
    Clear {
        paper: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
}

pub(super) async fn annotation(ctx: &Ctx, cmd: AnnotationCmd) -> Result<()> {
    let pool = &ctx.pool;
    let app = ctx.app();
    let svc = app.annotations();
    match cmd {
        AnnotationCmd::List { paper } => {
            let paper = papers::find_one(pool, &paper).await?;
            let items = svc.list(&paper.id).await?;
            if items.is_empty() {
                println!("no annotations");
            }
            for a in &items {
                // Pages are 0-based in storage and 1-based to a reader.
                println!(
                    "{}  p{}  {}/{}  {}",
                    a.id,
                    a.page_index + 1,
                    a.kind,
                    a.color,
                    ellipsize(a.quoted_text.as_deref().unwrap_or(""), 60)
                );
                if let Some(note) = &a.note {
                    println!("      note: {note}");
                }
            }
        }
        AnnotationCmd::Rm { paper, id } => {
            let paper = papers::find_one(pool, &paper).await?;
            if svc.delete(&paper.id, &id).await? {
                println!("removed {id}");
            } else {
                anyhow::bail!("no annotation {id} on {}", paper.id);
            }
        }
        AnnotationCmd::Clear { paper, yes } => {
            let paper = papers::find_one(pool, &paper).await?;
            let n = svc.list(&paper.id).await?.len();
            if n == 0 {
                println!("no annotations");
            } else if yes || confirm(&format!("Delete {n} annotation(s)?"))? {
                println!("removed {}", svc.delete_all(&paper.id).await?);
            }
        }
    }
    // The search index carries annotation notes; a CLI edit while
    // `serve` holds Tantivy's single-writer lock cannot reindex here
    // (see `index rebuild`). The running server's sweep picks the
    // change up on its next pass; a CLI-only session catches it the
    // next time one runs.
    Ok(())
}
