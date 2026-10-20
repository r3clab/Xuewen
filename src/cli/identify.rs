use anyhow::Result;

use super::output::{author_line, confirm};
use super::{in_trash, Ctx};
use crate::ingest::pipeline::IdentifyOutcome;
use crate::library::model::Identifier;

#[derive(clap::Args)]
pub(super) struct IdentifyArgs {
    /// Paper id (exact or unique prefix).
    id: String,
    /// Apply the Crossref record for this DOI.
    #[arg(long, conflicts_with_all = ["arxiv", "title"])]
    doi: Option<String>,
    /// Apply the arXiv record for this id.
    #[arg(long, conflicts_with_all = ["doi", "title"])]
    arxiv: Option<String>,
    /// Search DBLP/Crossref for this title and list candidates.
    #[arg(long, conflicts_with_all = ["doi", "arxiv"])]
    title: Option<String>,
    /// Apply candidate N from the --title list (1-based).
    #[arg(long, requires = "title")]
    pick: Option<usize>,
    /// Skip the confirmation prompt.
    #[arg(long)]
    yes: bool,
}

pub(super) async fn identify(ctx: &Ctx, args: IdentifyArgs) -> Result<()> {
    let IdentifyArgs {
        id,
        doi,
        arxiv,
        title,
        pick,
        yes,
    } = args;
    let app = ctx.app();
    let identify = app.identify()?;
    // Early UX check; the enforced guard lives in apply_match. This lets us
    // bail before the interactive search/confirm flow rather than after.
    let mut paper = ctx.active_paper(&id).await?;
    let md = if let Some(doi) = doi {
        identify
            .lookup(&Identifier::doi(doi.clone()))
            .await
            .ok_or_else(|| anyhow::anyhow!("no Crossref record for doi {doi} — try --title"))?
    } else if let Some(axv) = arxiv {
        identify
            .lookup(&Identifier::arxiv(axv.clone()))
            .await
            .ok_or_else(|| anyhow::anyhow!("no arXiv record for {axv} — try --title"))?
    } else if let Some(query) = title {
        let cands = identify.candidates(&query).await;
        if cands.is_empty() {
            anyhow::bail!("no candidates found for {query:?}");
        }
        match pick {
            Some(n) if n >= 1 && n <= cands.len() => cands.into_iter().nth(n - 1).unwrap(),
            Some(n) => anyhow::bail!("--pick {n} is out of range (1..={})", cands.len()),
            None => {
                for (i, c) in cands.iter().enumerate() {
                    println!(
                        "{:2}. {} — {} ({}, {}) [{}]",
                        i + 1,
                        c.title.as_deref().unwrap_or("(untitled)"),
                        author_line(&c.authors),
                        c.venue.as_deref().unwrap_or("?"),
                        c.year.map_or("?".to_string(), |y| y.to_string()),
                        c.source,
                    );
                }
                println!("re-run with --pick <N> to apply one");
                return Ok(());
            }
        }
    } else {
        anyhow::bail!("provide one of --doi, --arxiv, or --title");
    };

    println!(
        "match: {} — {} ({}, {})",
        md.title.as_deref().unwrap_or("(untitled)"),
        author_line(&md.authors),
        md.venue.as_deref().unwrap_or("?"),
        md.year.map_or("?".to_string(), |y| y.to_string()),
    );
    if yes || confirm("Apply this match?")? {
        match identify.apply(&mut paper, md).await? {
            IdentifyOutcome::Applied => println!(
                "identified {} as {}",
                paper.id,
                paper.cite_key.as_deref().unwrap_or("(no cite key)")
            ),
            IdentifyOutcome::SameWork(other) => {
                anyhow::bail!("that identifier already belongs to {other}")
            }
            IdentifyOutcome::Trashed => return Err(in_trash(&paper.id)),
        }
    } else {
        println!("cancelled");
    }
    Ok(())
}
