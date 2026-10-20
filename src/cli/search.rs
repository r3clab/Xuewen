use anyhow::Result;
use clap::Subcommand;

use super::output::strip_snippet_html;
use super::{remote, Ctx};
use crate::search::vector::OpenMode;
use crate::search::{indexer, IndexStatus, SearchOutcome, SearchService, Semantic, TierCounts};

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

/// What `xuewen search` prints, whether the search ran here or in the server
/// holding the vector store.
#[derive(Debug, PartialEq)]
struct SearchReport {
    semantic_reason: Option<String>,
    rows: Vec<Row>,
}

#[derive(Debug, PartialEq)]
struct Row {
    label: String,
    title: Option<String>,
    field: String,
    page: Option<i64>,
    snippet: String,
}

impl From<&SearchOutcome> for SearchReport {
    fn from(out: &SearchOutcome) -> Self {
        Self {
            semantic_reason: out.semantic.reason.clone(),
            rows: out
                .results
                .iter()
                .map(|(p, m)| Row {
                    label: p.cite_key.clone().unwrap_or_else(|| p.id.clone()),
                    title: p.meta.title.clone(),
                    field: m.field.to_string(),
                    page: m.page,
                    snippet: m.snippet.clone(),
                })
                .collect(),
        }
    }
}

impl From<remote::SearchReply> for SearchReport {
    fn from(reply: remote::SearchReply) -> Self {
        Self {
            semantic_reason: reply.semantic.reason,
            rows: reply
                .results
                .into_iter()
                .map(|h| Row {
                    label: h.paper.cite_key.unwrap_or(h.paper.id),
                    title: h.paper.title,
                    field: h.matched.field,
                    page: h.matched.page,
                    snippet: h.matched.snippet,
                })
                .collect(),
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
    let semantic = if keyword_only {
        Semantic::NotConfigured
    } else {
        Semantic::open(pool, &cfg.search, &cfg.ai, OpenMode::LoadOrCreate).await?
    };
    if let Some(addr) = semantic.holder() {
        let engines = semantic_only.then_some("semantic");
        match remote::search(addr, &query, fields.as_deref(), engines).await {
            Ok(reply) => {
                print_search(&SearchReport::from(reply), keyword_only);
                return Ok(());
            }
            Err(e) => eprintln!("note: could not reach xuewen at {addr} ({e:#}); searching here"),
        }
    }
    let svc = SearchService::open(pool.clone(), &cfg.search, semantic).await?;
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
    print_search(&SearchReport::from(&out), keyword_only);
    Ok(())
}

fn print_search(report: &SearchReport, keyword_only: bool) {
    if let Some(reason) = &report.semantic_reason {
        if !keyword_only {
            eprintln!("note: semantic search unavailable — {reason}");
        }
    }
    if report.rows.is_empty() {
        println!("no matches");
    }
    for (i, row) in report.rows.iter().enumerate() {
        println!(
            "{:2}. {}  {}",
            i + 1,
            row.label,
            row.title.as_deref().unwrap_or("(untitled)")
        );
        let loc = match row.page {
            Some(pg) => format!("{} p.{pg}", row.field),
            None => row.field.clone(),
        };
        println!("      [{loc}] {}", strip_snippet_html(&row.snippet));
    }
}

async fn status_report(ctx: &Ctx) -> Result<IndexStatus> {
    let (cfg, pool) = (&ctx.cfg, &ctx.pool);
    let semantic = Semantic::open(pool, &cfg.search, &cfg.ai, OpenMode::LoadOrCreate).await?;
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
    use crate::testutil::{paper, temp_pool};

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
                authors: String::new(),
                venue: String::new(),
                abstract_text: String::new(),
                body: "we fuzz routers".into(),
                notes: String::new(),
            })
            .unwrap();
        let svc = SearchService::open_with(pool.clone(), fts_idx, None);
        let mut app = crate::app::App::builder(pool.clone(), dir.path().to_path_buf());
        if with_search {
            app = app.search(svc.clone());
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(crate::web::serve_on(
            listener,
            app.build(),
            tokio_util::sync::CancellationToken::new(),
        ));
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
        let remote =
            SearchReport::from(remote::search(s.addr, "fuzzing", None, None).await.unwrap());

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
        let local = SearchReport::from(&s.svc.search(&req).await.unwrap());
        assert_eq!(remote, local);
        assert_eq!(local.rows.len(), 1);
        assert!(local.semantic_reason.is_some());
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
