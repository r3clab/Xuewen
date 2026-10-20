use anyhow::anyhow;
use anyhow::Result;
use std::path::Path;
use std::time::Duration;

use super::ResolvedMetadata;
use crate::text::collapse_ws;

/// Parse a GROBID `processHeaderDocument` TEI response into metadata.
/// Returns `Ok(None)` if no title, abstract, or authors could be found.
fn parse_tei(xml: &str) -> Result<Option<ResolvedMetadata>> {
    let doc = roxmltree::Document::parse(xml)?;
    let is = |n: &roxmltree::Node, name: &str| n.tag_name().name() == name;

    // Title: prefer <title type="main">, else the first <title>.
    let title = doc
        .descendants()
        .find(|n| is(n, "title") && n.attribute("type") == Some("main"))
        .or_else(|| doc.descendants().find(|n| is(n, "title")))
        .and_then(|n| n.text())
        .map(collapse_ws)
        .filter(|s| !s.is_empty());

    // Abstract: concatenate every <p> under <abstract>.
    let abstract_text = doc
        .descendants()
        .find(|n| is(n, "abstract"))
        .map(|ab| {
            ab.descendants()
                .filter(|n| is(n, "p"))
                .filter_map(|p| p.text().map(collapse_ws))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|s| !s.is_empty());

    // Authors: each <author>'s <persName> -> forename(s) + surname.
    let authors: Vec<String> = doc
        .descendants()
        .filter(|n| is(n, "author"))
        .filter_map(|a| {
            let pn = a.descendants().find(|n| is(n, "persName"))?;
            let parts: Vec<String> = pn
                .descendants()
                .filter(|n| is(n, "forename") || is(n, "surname"))
                .filter_map(|n| n.text().map(|t| t.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect();
            (!parts.is_empty()).then(|| parts.join(" "))
        })
        .collect();

    if title.is_none() && abstract_text.is_none() && authors.is_empty() {
        return Ok(None);
    }
    Ok(Some(ResolvedMetadata {
        title,
        abstract_text,
        authors,
        venue: None,
        year: None,
        doi: None,
        arxiv_id: None,
        dblp_key: None,
        url: None,
        source: "grobid".to_string(),
    }))
}

/// A GROBID service client.
///
/// Deliberately plain `reqwest` with no retry policy: GROBID is a local,
/// user-run service — if it's down, degrading to the no-GROBID path
/// immediately beats stalling ingest with retries.
pub struct Grobid {
    http: reqwest::Client,
    base: String,
}

impl Grobid {
    /// Build a client for the GROBID service at `base` (e.g. `http://localhost:8070`).
    pub fn new(base: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            http,
            base: base.to_string(),
        })
    }

    /// Upload the PDF to `processHeaderDocument` and parse the TEI header.
    /// Returns `Ok(None)` if GROBID found nothing usable.
    pub async fn extract_header(&self, pdf_path: &Path) -> Result<Option<ResolvedMetadata>> {
        let bytes = tokio::fs::read(pdf_path).await?;
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name("input.pdf")
            .mime_str("application/pdf")?;
        let form = reqwest::multipart::Form::new().part("input", part);

        let resp = self
            .http
            .post(format!("{}/api/processHeaderDocument", self.base))
            .multipart(form)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!("grobid HTTP {}", resp.status()));
        }
        let tei = resp.text().await?;
        parse_tei(&tei)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/grobid_bert.tei.xml"
    ));

    #[test]
    fn parses_tei_header() {
        let md = parse_tei(FIXTURE).unwrap().unwrap();
        assert_eq!(
            md.title.as_deref(),
            Some(
                "BERT: Pre-training of Deep Bidirectional Transformers for Language Understanding"
            )
        );
        assert_eq!(md.authors, vec!["Jacob Devlin", "Ming-Wei Chang"]);
        assert_eq!(
            md.abstract_text.as_deref(),
            Some("We introduce a new language representation model called BERT.")
        );
        assert_eq!(md.source, "grobid");
    }

    #[test]
    fn empty_tei_is_none() {
        let xml = r#"<TEI xmlns="http://www.tei-c.org/ns/1.0"><teiHeader/></TEI>"#;
        assert!(parse_tei(xml).unwrap().is_none());
    }
}
