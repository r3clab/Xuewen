use std::net::SocketAddr;

use anyhow::{anyhow, bail, Context as _, Result};
use clap::Subcommand;

use super::remote::{self, DailyStart};
use super::{freshness, Ctx};
use crate::daily::store::{self, DailyRun};
use crate::daily::{scheduler, DailyService, Disabled};
use crate::search::indexer::{self, Scope};
use crate::search::{SearchService, Semantic};

#[derive(Subcommand)]
pub(super) enum DailyCmd {
    /// Show the latest run and its ranked arXiv recommendations.
    Show,
    /// Fetch, rank and summarize today's arXiv announcements now (needs
    /// [daily], [ai.embedding] and [ai.daily]).
    Run {
        /// Run even if today's batch already finished.
        #[arg(long)]
        force: bool,
    },
}

pub(super) async fn daily(ctx: &Ctx, cmd: DailyCmd) -> Result<()> {
    match cmd {
        DailyCmd::Show => show(ctx).await,
        DailyCmd::Run { force } => run(ctx, force).await,
    }
}

async fn show(ctx: &Ctx) -> Result<()> {
    let run = store::latest_run(&ctx.pool).await?;
    let batch = store::latest_batch(&ctx.pool).await?;
    if run.is_none() && batch.is_none() {
        println!("no daily runs yet — run: xuewen daily run");
        return Ok(());
    }
    if let Some(run) = &run {
        println!("{}", run_line(run));
    }
    let Some((date, papers)) = batch else {
        return Ok(());
    };
    if run.is_some_and(|r| r.batch_date != date) {
        println!("latest papers, from {date}:");
    }
    for p in papers {
        println!(
            "{:2}. {}  {}  ({:.3})",
            p.rank, p.arxiv_id, p.title, p.score
        );
        if let Some(tldr) = &p.tldr {
            println!("      {tldr}");
        }
    }
    Ok(())
}

async fn run(ctx: &Ctx, force: bool) -> Result<()> {
    let today = scheduler::today();
    if !force {
        let done = store::get_run(&ctx.pool, &today)
            .await?
            .filter(|r| scheduler::settled(Some(&r.status)));
        if let Some(run) = done {
            println!("{}", run_line(&run));
            println!("today's run already finished — pass --force to run it again");
            return Ok(());
        }
    }
    let semantic = ctx.open_semantic().await?;
    match semantic.holder() {
        Some(addr) => forward(addr).await,
        None => run_here(ctx, semantic, &today).await,
    }
}

async fn forward(addr: SocketAddr) -> Result<()> {
    let started = remote::start_daily(addr)
        .await
        .with_context(|| format!("asking the xuewen server at http://{addr} to run"))?;
    match started {
        DailyStart::Started => println!(
            "started today's run in the xuewen server at http://{addr} — follow it with: xuewen daily show"
        ),
        DailyStart::AlreadyRunning => {
            println!("the xuewen server at http://{addr} is already running today's batch")
        }
    }
    Ok(())
}

/// The day's papers are ranked against the library's embeddings, and no
/// other CLI command but `search` writes those, so the index is caught up
/// in full first.
async fn run_here(ctx: &Ctx, semantic: Semantic, today: &str) -> Result<()> {
    let daily = DailyService::try_from_config(&ctx.cfg, ctx.pool.clone(), &semantic)
        .and_then(|d| d.map_err(|Disabled(reason)| anyhow!("daily papers are off: {reason}")));
    let search = SearchService::open(ctx.pool.clone(), &ctx.cfg.search, semantic).await?;
    let run = async {
        let daily = daily?;
        freshness::report(indexer::catch_up(&search, &ctx.cfg.library_root, Scope::Full).await);
        eprintln!("running the daily batch for {today}…");
        daily
            .run_guarded(today)
            .await
            .ok_or_else(|| anyhow!("a daily run is already in flight"))
    }
    .await;
    search.close_vectors().await;
    let run = run?;
    if run.status == "failed" {
        bail!(
            "daily run {today} failed: {}",
            run.error.as_deref().unwrap_or("no detail recorded")
        );
    }
    println!("{}", run_line(&run));
    Ok(())
}

fn run_line(run: &DailyRun) -> String {
    let mut line = format!("{run}, ran {}", run.ran_at);
    if let Some(error) = &run.error {
        line.push_str(&format!(" — {error}"));
    }
    line
}
