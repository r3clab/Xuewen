use crate::support;

use std::io::Write;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use xuewen::metadata::resolve::grobid::Grobid;

const TEI_FIXTURE: &str = support::fixture!("grobid_bert.tei.xml");

#[tokio::test]
async fn extract_header_posts_pdf_and_parses_tei() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/processHeaderDocument"))
        .respond_with(ResponseTemplate::new(200).set_body_string(TEI_FIXTURE))
        .mount(&server)
        .await;

    // Any file works; the mock ignores the uploaded bytes.
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(b"%PDF-1.4 dummy").unwrap();

    let grobid = Grobid::new(&server.uri()).unwrap();
    let md = grobid.extract_header(f.path()).await.unwrap().unwrap();

    assert_eq!(
        md.title.as_deref(),
        Some("BERT: Pre-training of Deep Bidirectional Transformers for Language Understanding")
    );
    assert_eq!(md.authors, vec!["Jacob Devlin", "Ming-Wei Chang"]);
    assert_eq!(md.source, "grobid");
}

#[tokio::test]
async fn extract_header_errors_on_non_2xx() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/processHeaderDocument"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(b"%PDF-1.4 dummy").unwrap();

    let grobid = Grobid::new(&server.uri()).unwrap();
    assert!(grobid.extract_header(f.path()).await.is_err());
}
