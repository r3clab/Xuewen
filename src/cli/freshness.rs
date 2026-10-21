use super::Ctx;
use crate::search::indexer::{self, CatchUp, Scope};

/// FTS-only so a quick command never waits on the embedding API; the
/// vector tier follows on the next `search`, `daily run`, `watch` or
/// `serve`. Never changes the command's outcome.
pub(super) async fn settle(ctx: &Ctx) {
    if !ctx.app.take_index_dirty() {
        return;
    }
    let cfg = &ctx.cfg;
    report(
        indexer::catch_up_fresh(
            ctx.pool.clone(),
            &cfg.search,
            &cfg.ai,
            &cfg.library_root,
            Scope::FtsOnly,
        )
        .await,
    );
}

pub(super) fn report(caught_up: anyhow::Result<CatchUp>) {
    match caught_up {
        Ok(CatchUp::Swept(s)) if s.failed > 0 => eprintln!(
            "note: {} paper(s) could not be indexed — see: xuewen index status",
            s.failed
        ),
        Ok(CatchUp::Swept(_)) => {}
        Ok(CatchUp::Skipped(why)) => {
            eprintln!("note: search index not updated — {why}; it catches up on its next sweep")
        }
        Err(e) => eprintln!("warning: could not update the search index: {e:#}"),
    }
}
