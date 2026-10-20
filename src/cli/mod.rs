mod annotation;
mod code;
mod identify;
mod ingest;
mod library;
mod output;
mod project;
mod search;
mod serve;
mod settings;
mod summarize;
mod tag;

use anyhow::Result;
use clap::{Parser, Subcommand};
use sqlx::SqlitePool;
use std::path::PathBuf;

use crate::app::App;
use crate::config::Config;
use crate::db;
use crate::library::model::Paper;
use crate::library::papers;

#[derive(Parser)]
#[command(name = "xuewen", version)]
struct Cli {
    /// Path to the TOML config file.
    #[arg(long, default_value = "xuewen.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Ingest a single PDF file.
    Ingest(ingest::IngestArgs),
    /// Import a paper from a URL, DOI, or arXiv id.
    Import(ingest::ImportArgs),
    /// Manage the stored EZproxy session cookie used for paywalled imports.
    ProxyCookie(settings::ProxyCookieArgs),
    /// Watch the inbox directory and auto-ingest new PDFs (runs until stopped).
    Watch,
    /// Re-resolve failed records and re-file every paper to its cite-key path.
    Refresh(ingest::RefreshArgs),
    /// Manually match a paper to a DOI, arXiv id, or searched title.
    Identify(identify::IdentifyArgs),
    /// Serve the web UI over HTTP (loopback by default).
    Serve(serve::ServeArgs),
    /// Soft-delete a paper: hide it from the library (recoverable).
    Delete(library::DeleteArgs),
    /// Restore a trashed paper back into the library.
    Restore(library::RestoreArgs),
    /// Permanently remove trashed papers and their PDF files.
    Purge(library::PurgeArgs),
    /// Manage projects (named groups of related papers).
    Project {
        #[command(subcommand)]
        cmd: project::ProjectCmd,
    },
    /// Manage topical tags.
    Tag {
        #[command(subcommand)]
        cmd: tag::TagCmd,
    },
    /// Attach, inspect, or detach a paper's code repository
    Code {
        #[command(subcommand)]
        cmd: code::CodeCmd,
    },
    /// Inspect or clear a paper's reader annotations.
    Annotation {
        #[command(subcommand)]
        cmd: annotation::AnnotationCmd,
    },
    /// Star a paper.
    Star(library::StarArgs),
    /// Un-star a paper.
    Unstar(library::StarArgs),
    /// Export papers as BibTeX or BibLaTeX.
    Export(library::ExportArgs),
    /// Search the library from the terminal.
    Search(search::SearchArgs),
    /// Generate LLM summaries for library papers (needs [ai.summary]).
    Summarize(summarize::SummarizeArgs),
    /// Inspect or rebuild the search indexes.
    Index {
        #[command(subcommand)]
        cmd: search::IndexCmd,
    },
}

struct Ctx {
    cfg: Config,
    pool: SqlitePool,
}

impl Ctx {
    fn app(&self) -> App {
        App::for_cli(&self.cfg, self.pool.clone())
    }

    async fn active_paper(&self, selector: &str) -> Result<Paper> {
        let paper = papers::find_one(&self.pool, selector).await?;
        if paper.deleted_at.is_some() {
            return Err(in_trash(&paper.id));
        }
        Ok(paper)
    }
}

fn in_trash(id: &str) -> anyhow::Error {
    anyhow::anyhow!("{id} is in the trash — run: xuewen restore {id}")
}

#[tokio::main]
pub async fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let cfg = Config::load(&cli.config)?;
    let pool = db::connect(&cfg.database_url).await?;
    let ctx = Ctx { cfg, pool };

    match cli.command {
        Command::Ingest(args) => ingest::ingest(&ctx, args).await,
        Command::Import(args) => ingest::import(&ctx, args).await,
        Command::ProxyCookie(args) => settings::proxy_cookie(&ctx, args).await,
        Command::Watch => ingest::watch(&ctx).await,
        Command::Refresh(args) => ingest::refresh(&ctx, args).await,
        Command::Identify(args) => identify::identify(&ctx, args).await,
        Command::Serve(args) => serve::serve(&ctx, args).await,
        Command::Delete(args) => library::delete(&ctx, args).await,
        Command::Restore(args) => library::restore(&ctx, args).await,
        Command::Purge(args) => library::purge(&ctx, args).await,
        Command::Project { cmd } => project::project(&ctx, cmd).await,
        Command::Tag { cmd } => tag::tag(&ctx, cmd).await,
        Command::Code { cmd } => code::code(&ctx, cmd).await,
        Command::Annotation { cmd } => annotation::annotation(&ctx, cmd).await,
        Command::Star(args) => library::star(&ctx, args, true).await,
        Command::Unstar(args) => library::star(&ctx, args, false).await,
        Command::Export(args) => library::export(&ctx, args).await,
        Command::Search(args) => search::search(&ctx, args).await,
        Command::Summarize(args) => summarize::summarize(&ctx, args).await,
        Command::Index { cmd } => search::index(&ctx, cmd).await,
    }
}
