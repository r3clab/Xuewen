use printpdf::*;
use std::path::{Path, PathBuf};
use std::time::Duration;

use xuewen::app::App;

use xuewen::ingest::fetch::Fetcher;
use xuewen::ingest::pipeline::{IngestCtx, Libraries};
use xuewen::ingest::Ingest;
use xuewen::metadata::resolve::Resolver;
pub use xuewen::testutil::{sample_paper, temp_pool};

macro_rules! fixture {
    ($n:literal) => {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/", $n))
    };
}
pub(crate) use fixture;

/// Write a one-page PDF whose lines are `lines`, top-to-bottom.
/// pdftotext reliably extracts built-in Helvetica text.
pub fn write_test_pdf(path: &Path, lines: &[&str]) {
    let mut doc = PdfDocument::new("test");
    let mut ops = vec![
        Op::StartTextSection,
        Op::SetFont {
            font: PdfFontHandle::Builtin(BuiltinFont::Helvetica),
            size: Pt(12.0),
        },
        Op::SetLineHeight { lh: Pt(18.0) },
        Op::SetTextCursor {
            pos: Point::new(Mm(15.0), Mm(280.0)),
        },
    ];
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            ops.push(Op::AddLineBreak);
        }
        ops.push(Op::ShowText {
            items: vec![TextItem::Text((*line).to_string())],
        });
    }
    ops.push(Op::EndTextSection);
    let page = PdfPage::new(Mm(210.0), Mm(297.0), ops);
    let bytes = doc
        .with_pages(vec![page])
        .save(&PdfSaveOptions::default(), &mut Vec::new());
    std::fs::write(path, bytes).unwrap();
}

/// The pool and library root live in the returned temp dir, which the caller
/// keeps alive for as long as it uses them.
pub async fn pool_and_root_with_paper(id: &str) -> (sqlx::SqlitePool, PathBuf, tempfile::TempDir) {
    let (pool, dir) = xuewen::testutil::pool_with_paper(id).await;
    let library_root = dir.path().join("library");
    std::fs::create_dir_all(&library_root).unwrap();
    (pool, library_root, dir)
}

/// `data_dir` is `dir`, so the database is the one `temp_pool` creates there.
pub fn write_config(dir: &Path, extra: &str) -> PathBuf {
    let data_dir = toml::Value::from(dir.display().to_string());
    let path = dir.join("xuewen.toml");
    std::fs::write(&path, format!("data_dir = {data_dir}\n{extra}")).unwrap();
    path
}

pub fn test_cfg(dir: &Path) -> xuewen::config::Config {
    test_cfg_with(dir, "")
}

pub fn test_cfg_with(dir: &Path, extra: &str) -> xuewen::config::Config {
    xuewen::config::Config::load(&write_config(dir, extra)).unwrap()
}

pub fn ingest(pool: sqlx::SqlitePool, dir: &Path, resolver: Resolver, fetcher: Fetcher) -> Ingest {
    Ingest::new(
        IngestCtx::new(
            pool,
            Libraries::under(&dir.join("inbox"), &dir.join("library")),
            resolver,
            None,
        ),
        fetcher,
    )
}

pub fn router(pool: sqlx::SqlitePool, library_root: PathBuf) -> axum::Router {
    xuewen::web::router(App::builder(pool, library_root).build())
}

pub async fn eventually(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    cond()
}

pub fn stub_runner_path() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/stub_runner.mjs"
    ))
}
