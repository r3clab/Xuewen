use anyhow::Result;
use clap::Subcommand;

use super::output::confirm;
use super::Ctx;
use crate::agent::history;

#[derive(Subcommand)]
pub(super) enum ChatCmd {
    /// Print a paper's saved Ask conversation.
    History { paper: String },
    /// Delete a paper's saved Ask conversation.
    Clear {
        paper: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
    /// List the agent backends `xuewen ask --model` accepts.
    Models,
}

pub(super) async fn chat(ctx: &Ctx, cmd: ChatCmd) -> Result<()> {
    match cmd {
        ChatCmd::History { paper } => show_history(ctx, &paper).await,
        ChatCmd::Clear { paper, yes } => clear(ctx, &paper, yes).await,
        ChatCmd::Models => {
            models(ctx);
            Ok(())
        }
    }
}

async fn show_history(ctx: &Ctx, paper: &str) -> Result<()> {
    let paper = ctx.active_paper(paper).await?;
    let rows = history::list(&ctx.pool, &paper.id).await?;
    if rows.is_empty() {
        println!(
            "no conversation yet — start one with: xuewen ask {} \"<question>\"",
            paper.id
        );
    }
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            println!();
        }
        let who = match row.role.as_str() {
            "user" => "you",
            _ => row.model.as_deref().unwrap_or("agent"),
        };
        println!("{who}, {}:", row.created_at);
        for tool in row.tools().unwrap_or_default() {
            println!("[{}] {}", tool.name, tool.detail);
        }
        println!("{}", row.content.trim_end());
    }
    Ok(())
}

async fn clear(ctx: &Ctx, paper: &str, yes: bool) -> Result<()> {
    let paper = ctx.active_paper(paper).await?;
    let n = history::count(&ctx.pool, &paper.id).await?;
    if n == 0 {
        println!("no conversation");
    } else if yes || confirm(&format!("Delete {n} message(s)?"))? {
        history::clear(&ctx.pool, &paper.id).await?;
        println!("removed {n}");
    } else {
        println!("cancelled");
    }
    Ok(())
}

fn models(ctx: &Ctx) {
    let agent = match ctx.app.agent() {
        Ok(agent) => agent,
        Err(e) => {
            println!("{e}");
            return;
        }
    };
    let default = &agent.default_backend().id;
    for b in &agent.backends {
        let model = b
            .model
            .as_deref()
            .map(|m| format!(", {m}"))
            .unwrap_or_default();
        let marker = if &b.id == default { "  (default)" } else { "" };
        println!("{}  {}{model}{marker}", b.id, b.label);
    }
}
