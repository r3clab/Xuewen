use anyhow::Result;

use super::Ctx;
use crate::app::summarize::SummarizeScope;

#[derive(clap::Args)]
pub(super) struct SummarizeArgs {
    /// Paper id (exact) to (re)summarize. Omit to fill gaps for the whole library.
    #[arg(conflicts_with = "all")]
    id: Option<String>,
    /// Clear and regenerate every paper's summary.
    #[arg(long)]
    all: bool,
}

pub(super) async fn summarize(ctx: &Ctx, args: SummarizeArgs) -> Result<()> {
    let SummarizeArgs { id, all } = args;
    let scope = match (all, id) {
        (true, _) => SummarizeScope::All,
        (false, Some(id)) => SummarizeScope::Paper(id),
        (false, None) => SummarizeScope::Pending,
    };
    let one = matches!(scope, SummarizeScope::Paper(_));
    let generated = ctx.app.summarize(scope).await?;
    if one {
        println!(
            "{}",
            if generated > 0 {
                "summary generated"
            } else {
                "no summary generated (see logs)"
            }
        );
    } else {
        println!("generated {generated} summaries");
    }
    Ok(())
}
