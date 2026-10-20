//! End-to-end: import a PDF -> background sweep -> find it by a body phrase.

use crate::support;

use printpdf::*;
use std::path::Path;

use xuewen::ingest::pipeline::{IngestCtx, Libraries};
use xuewen::search::{fts, indexer, SearchRequest, SearchService};

/// Two text lines: a title-looking first line, then the body phrase. If the
/// ingest heuristics take the first line as the title, the search phrase
/// still only exists in the body — so the snippet's field must be "body".
fn write_pdf(path: &Path, title_line: &str, body_line: &str) {
    let mut doc = PdfDocument::new("t");
    let ops = vec![
        Op::StartTextSection,
        Op::SetFont {
            font: PdfFontHandle::Builtin(BuiltinFont::Helvetica),
            size: Pt(14.0),
        },
        Op::SetLineHeight { lh: Pt(30.0) },
        Op::SetTextCursor {
            pos: Point::new(Mm(15.0), Mm(280.0)),
        },
        Op::ShowText {
            items: vec![TextItem::Text(title_line.to_string())],
        },
        Op::AddLineBreak,
        Op::ShowText {
            items: vec![TextItem::Text(body_line.to_string())],
        },
        Op::EndTextSection,
    ];
    let page = PdfPage::new(Mm(210.0), Mm(297.0), ops);
    let bytes = doc
        .with_pages(vec![page])
        .save(&PdfSaveOptions::default(), &mut Vec::new());
    std::fs::write(path, bytes).unwrap();
}

#[tokio::test]
async fn imported_pdf_becomes_keyword_searchable_by_body_text() {
    let dir = tempfile::tempdir().unwrap();
    let library_root = dir.path().join("library");
    let inbox = dir.path().join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = xuewen::db::connect(&url).await.unwrap();

    // 1. Ingest a PDF whose body holds a distinctive phrase.
    let pdf = inbox.join("paper.pdf");
    write_pdf(
        &pdf,
        "A Study of Distributed Authorization",
        "we evaluate the zanzibar consistency protocol",
    );
    let ctx = IngestCtx::new(
        pool.clone(),
        Libraries::under(&inbox, &library_root),
        // Upstreams refuse instantly -> the paper lands as needs_review, offline.
        support::offline_resolver(),
        None,
    );
    ctx.ingest_file(&pdf).await.unwrap();

    // 2. One indexer sweep (keyword tier only; no embedder configured).
    let idx_dir = dir.path().join("search-index");
    let (fts_idx, _) = fts::FtsIndex::open(&idx_dir).unwrap();
    let svc = SearchService::open_with(pool, fts_idx, None);
    let summary = indexer::sweep(&svc, &library_root).await.unwrap();
    assert_eq!(summary.indexed, 1);
    assert_eq!(summary.failed, 0);

    // 3. A body phrase finds the paper, with an evidence snippet.
    let out = svc
        .search(&SearchRequest {
            q: "zanzibar".into(),
            author_terms: Vec::new(),
            fields: fts::FieldSel::all(),
            keyword: true,
            semantic: true,
            status: None,
            project: None,
            tag: None,
            starred: None,
        })
        .await
        .unwrap();
    assert!(!out.semantic.available, "no embedder configured");
    assert_eq!(out.results.len(), 1);
    let (paper, m) = &out.results[0];
    assert!(paper.rel_path.ends_with(".pdf"));
    assert_eq!(m.field, fts::FieldName::Body);
    assert!(
        m.snippet.contains("<mark>zanzibar</mark>"),
        "got: {}",
        m.snippet
    );

    // 4. Status agrees everything is indexed.
    let st = svc.status().await.unwrap();
    assert_eq!(st.fts.pending, 0);
    assert_eq!(st.fts.failed, 0);
}
