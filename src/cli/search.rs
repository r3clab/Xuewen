use anyhow::Result;
use clap::Subcommand;

use serde::Deserialize;

use super::output;
use super::{freshness, remote, Ctx};
use crate::library::model::Paper;
use crate::search::indexer::{self, Scope};
use crate::search::{IndexStatus, SearchResults, SearchService, TierCounts};

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
        /// Rebuild only the vector store (recreates it).
        #[arg(long)]
        vectors_only: bool,
    },
}

/// The paper fields `xuewen search` prints, whether the search ran here or
/// in the server holding the vector store (whose rows carry more, ignored).
#[derive(Debug, PartialEq, Deserialize)]
pub(super) struct PaperLabel {
    id: String,
    cite_key: Option<String>,
    title: Option<String>,
}

impl From<Paper> for PaperLabel {
    fn from(p: Paper) -> Self {
        Self {
            id: p.id,
            cite_key: p.cite_key,
            title: p.meta.title,
        }
    }
}

pub(super) async fn search(ctx: &Ctx, args: SearchArgs) -> Result<()> {
    let SearchArgs {
        query,
        fields,
        keyword_only,
        semantic_only,
    } = args;
    let (cfg, pool) = (&ctx.cfg, &ctx.pool);
    // Opened even for --keyword-only: the catch-up below must see the real
    // store, or deindexing a trashed paper would orphan its vectors.
    let semantic = ctx.open_semantic().await?;
    if let Some(addr) = semantic.holder() {
        let engines = semantic_only
            .then_some("semantic")
            .or(keyword_only.then_some("keyword"));
        match remote::search(addr, &query, fields.as_deref(), engines).await {
            Ok(reply) => {
                print_search(&reply, keyword_only);
                return Ok(());
            }
            Err(e) => eprintln!("note: could not reach xuewen at {addr} ({e:#}); searching here"),
        }
    }
    let vectors = semantic.store().cloned();
    let svc = SearchService::open(pool.clone(), &cfg.search, semantic).await?;
    let scope = if keyword_only {
        Scope::FtsOnly
    } else {
        Scope::Full
    };
    freshness::report(indexer::catch_up(&svc, &cfg.library_root, scope).await);
    let out = search_here(&svc, pool, &query, fields, keyword_only, semantic_only).await;
    if let Some(vectors) = vectors {
        vectors.close_gracefully().await;
    }
    print_search(&out?.map_papers(PaperLabel::from), keyword_only);
    Ok(())
}

async fn search_here(
    svc: &SearchService,
    pool: &sqlx::SqlitePool,
    query: &str,
    fields: Option<String>,
    keyword_only: bool,
    semantic_only: bool,
) -> Result<SearchResults<Paper>> {
    let req = crate::search::SearchRequest::assemble(
        pool,
        query,
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
    svc.search(&req).await
}

fn print_search(report: &SearchResults<PaperLabel>, keyword_only: bool) {
    if let Some(reason) = &report.semantic.reason {
        if !keyword_only {
            eprintln!("note: semantic search unavailable — {reason}");
        }
    }
    if report.results.is_empty() {
        println!("no matches");
    }
    for (i, hit) in report.results.iter().enumerate() {
        let paper = &hit.paper;
        println!(
            "{:2}. {}  {}",
            i + 1,
            paper.cite_key.as_deref().unwrap_or(&paper.id),
            output::title(paper.title.as_deref())
        );
        let m = &hit.matched;
        let loc = match m.page {
            Some(pg) => format!("{} p.{pg}", m.field),
            None => m.field.to_string(),
        };
        println!("      [{loc}] {}", m.snippet.text());
    }
}

async fn status_report(ctx: &Ctx) -> Result<IndexStatus> {
    let (cfg, pool) = (&ctx.cfg, &ctx.pool);
    let semantic = ctx.open_semantic().await?;
    if let Some(addr) = semantic.holder() {
        match remote::status(addr).await {
            Ok(status) => return Ok(status),
            Err(e) => eprintln!("note: could not reach xuewen at {addr} ({e:#}); reading here"),
        }
    }
    let svc = SearchService::open(pool.clone(), &cfg.search, semantic).await?;
    svc.status().await
}

fn print_status(st: &IndexStatus) {
    let TierCounts {
        indexed,
        pending,
        failed,
    } = st.fts;
    println!("full-text: {indexed} indexed, {pending} pending, {failed} failed");
    let TierCounts {
        indexed,
        pending,
        failed,
    } = st.vectors;
    println!("vectors:   {indexed} indexed, {pending} pending, {failed} failed");
    match &st.reason {
        None => println!("semantic search: available"),
        Some(r) => println!("semantic search: unavailable — {r}"),
    }
}

pub(super) async fn index(ctx: &Ctx, cmd: IndexCmd) -> Result<()> {
    let (cfg, pool) = (&ctx.cfg, &ctx.pool);
    match cmd {
        IndexCmd::Status => print_status(&status_report(ctx).await?),
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

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::Arc;

    use super::*;
    use crate::library::model::PaperStatus;
    use crate::search::{fts, RequestOverrides, SearchRequest};
    use crate::testutil::{paper, serve, temp_pool};

    struct Served {
        _dir: tempfile::TempDir,
        pool: sqlx::SqlitePool,
        svc: Arc<SearchService>,
        addr: SocketAddr,
    }

    async fn served(with_search: bool) -> Served {
        let (dir, pool) = temp_pool().await;
        crate::library::papers::insert(
            &pool,
            &paper("a", "Fuzzing Firmware", PaperStatus::Resolved),
        )
        .await
        .unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(&dir.path().join("index")).unwrap();
        fts_idx
            .upsert(&fts::PaperDoc {
                id: "a".into(),
                title: "Fuzzing Firmware".into(),
                body: "we fuzz routers".into(),
                ..Default::default()
            })
            .unwrap();
        let svc = SearchService::open_with(pool.clone(), fts_idx, None);
        let mut app = crate::app::App::builder(pool.clone(), dir.path().to_path_buf());
        if with_search {
            app = app.search(svc.clone());
        }
        let addr = serve(app.build()).await;
        Served {
            _dir: dir,
            pool,
            svc,
            addr,
        }
    }

    #[tokio::test]
    async fn a_forwarded_search_reports_what_a_local_one_does() {
        let s = served(true).await;
        let remote = remote::search(s.addr, "fuzzing", None, None).await.unwrap();

        let req = SearchRequest::assemble(
            &s.pool,
            "fuzzing",
            RequestOverrides {
                keyword: true,
                semantic: true,
                fields: None,
                status: None,
                project: None,
                tag: None,
                starred: None,
            },
        )
        .await
        .unwrap();
        let local = s
            .svc
            .search(&req)
            .await
            .unwrap()
            .map_papers(PaperLabel::from);
        assert_eq!(remote, local);
        assert_eq!(local.results.len(), 1);
        assert!(local.semantic.reason.is_some());
    }

    #[tokio::test]
    async fn a_forwarded_status_reports_what_a_local_one_does() {
        let s = served(true).await;
        assert_eq!(
            remote::status(s.addr).await.unwrap(),
            s.svc.status().await.unwrap()
        );
    }

    #[tokio::test]
    async fn a_server_without_search_is_an_error_not_an_empty_result() {
        let s = served(false).await;
        let err = remote::search(s.addr, "fuzzing", None, None)
            .await
            .err()
            .unwrap();
        assert!(err.to_string().contains("503"), "got: {err}");
    }
}
