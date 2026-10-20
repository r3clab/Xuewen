use anyhow::Result;
use clap::Subcommand;

use super::Ctx;
use crate::agent::store::{self, CodeStatus};
use crate::app::code::CloneMode;

#[derive(Subcommand)]
pub(super) enum CodeCmd {
    /// Attach a repo (https URL) — clones now, pinned to HEAD
    Set { paper: String, url: String },
    /// Show the attached repo and its clone status
    Status { paper: String },
    /// Detach the repo and delete the local checkout
    Rm { paper: String },
}

pub(super) async fn code(ctx: &Ctx, cmd: CodeCmd) -> Result<()> {
    let app = ctx.app();
    let pool = &ctx.pool;
    match cmd {
        CodeCmd::Set { paper, url } => {
            let paper = ctx.active_paper(&paper).await?;
            // CLI clones inline so the outcome prints immediately.
            match app.attach_code(&paper, &url, CloneMode::Foreground).await? {
                Some(c) if c.status == CodeStatus::Ready => {
                    println!(
                        "attached {} at {}",
                        c.repo_url,
                        c.commit_sha.as_deref().unwrap_or("?")
                    )
                }
                Some(c) => println!(
                    "attach failed: {}",
                    c.error.as_deref().unwrap_or("unknown error")
                ),
                None => println!("attach failed: no record"),
            }
        }
        CodeCmd::Status { paper } => {
            let paper = ctx.active_paper(&paper).await?;
            match store::get_paper_code(pool, &paper.id).await? {
                None => println!("no repo attached"),
                Some(c) => println!(
                    "{} — {}{}",
                    c.repo_url,
                    c.status,
                    c.commit_sha.map(|s| format!(" @ {s}")).unwrap_or_default()
                ),
            }
        }
        CodeCmd::Rm { paper } => {
            let paper = ctx.active_paper(&paper).await?;
            app.detach_code(&paper).await?;
            println!("detached");
        }
    }
    Ok(())
}
