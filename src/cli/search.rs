use anyhow::Result;
use clap::Subcommand;

use super::output::strip_snippet_html;
use super::Ctx;
use crate::search::{indexer, SearchService};

#[derive(clap::Args)]
pub(super) struct SearchArgs {
    /// Query; supports tag:/project:/is:starred/status:/in:/author: qualifiers
    /// and "quoted phrases" (e.g. 'tag:nlp author:smith attention').
    query: String,
    /// Comma-separated fields: title,authors,abstract,body,notes (default all).
    #[arg(long)]
    fields: Option<String>,
    /// Keyword (BM25) engine only.
    #[arg(long, conflicts_with = "semantic_only")]
    keyword_only: bool,
    /// Semantic (embedding) engine only.
    #[arg(long)]
    semantic_only: bool,
}

#[derive(Subcommand)]
pub(super) enum IndexCmd {
    /// Show per-tier indexing counts.
    Status,
    /// Drop and re-derive the search indexes from SQLite + PDFs (stop `xuewen serve` first).
    Rebuild {
        /// Rebuild only the Tantivy full-text index.
        #[arg(long, conflicts_with = "vectors_only")]
        fts_only: bool,
        /// Rebuild only the Qdrant vectors (recreates the collection).
        #[arg(long)]
        vectors_only: bool,
    },
}

pub(super) async fn search(ctx: &Ctx, args: SearchArgs) -> Result<()> {
    let SearchArgs {
        query,
        fields,
        keyword_only,
        semantic_only,
    } = args;
    let (cfg, pool) = (&ctx.cfg, &ctx.pool);
    let svc = SearchService::open(pool.clone(), &cfg.search, &cfg.ai).await?;
    let req = crate::search::SearchRequest::assemble(
        pool,
        &query,
        crate::search::RequestOverrides {
            keyword: !semantic_only,
            semantic: !keyword_only,
            fields,
            status: None,
            project: None,
            tag: None,
            starred: None,
        },
    )
    .await?;
    let out = svc.search(&req).await?;
    if let Some(reason) = &out.semantic.reason {
        if !keyword_only {
            eprintln!("note: semantic search unavailable — {reason}");
        }
    }
    if out.results.is_empty() {
        println!("no matches");
    }
    for (i, (p, m)) in out.results.iter().enumerate() {
        let label = p.cite_key.as_deref().unwrap_or(&p.id);
        println!(
            "{:2}. {}  {}",
            i + 1,
            label,
            p.meta.title.as_deref().unwrap_or("(untitled)")
        );
        let loc = match m.page {
            Some(pg) => format!("{} p.{pg}", m.field),
            None => m.field.to_string(),
        };
        println!("      [{loc}] {}", strip_snippet_html(&m.snippet));
    }
    Ok(())
}

pub(super) async fn index(ctx: &Ctx, cmd: IndexCmd) -> Result<()> {
    let (cfg, pool) = (&ctx.cfg, &ctx.pool);
    match cmd {
        IndexCmd::Status => {
            let svc = SearchService::open(pool.clone(), &cfg.search, &cfg.ai).await?;
            let st = svc.status().await?;
            println!(
                "full-text: {} indexed, {} pending, {} failed",
                st.fts.indexed, st.fts.pending, st.fts.failed
            );
            println!(
                "vectors:   {} indexed, {} pending, {} failed",
                st.vectors.indexed, st.vectors.pending, st.vectors.failed
            );
            match st.reason {
                None => println!("semantic search: available"),
                Some(r) => println!("semantic search: unavailable — {r}"),
            }
        }
        IndexCmd::Rebuild {
            fts_only,
            vectors_only,
        } => {
            let scope = match (fts_only, vectors_only) {
                (true, false) => indexer::RebuildScope::Fts,
                (false, true) => indexer::RebuildScope::Vectors,
                // clap's conflicts_with forbids both flags at once.
                _ => indexer::RebuildScope::Both,
            };
            let s = indexer::rebuild(pool.clone(), &cfg.search, &cfg.ai, &cfg.library_root, scope)
                .await?;
            println!(
                "rebuild: {} indexed, {} removed, {} failed",
                s.indexed, s.deindexed, s.failed
            );
            if s.failed > 0 {
                anyhow::bail!("some papers failed to index — see the log; re-run to retry");
            }
        }
    }
    Ok(())
}
