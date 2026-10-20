use anyhow::Result;

use super::ResolvedMetadata;
use crate::http::HttpClient;
use crate::text::collapse_ws;

/// Fetch the Atom response for a single arXiv id from `{base}/api/query`.
pub(crate) async fn fetch(http: &HttpClient, base: &str, id: &str) -> Result<String> {
    // `.query()` percent-encodes the id; a hand-formatted `?id_list={id}`
    // would embed user-supplied characters raw.
    let req = http
        .get(&format!("{base}/api/query"))
        .query(&[("id_list", id)]);
    http.send_text(req).await
}

/// Parse an arXiv Atom feed into metadata. Returns `Ok(None)` if there is no entry.
pub(crate) fn parse(atom: &str) -> Result<Option<ResolvedMetadata>> {
    let doc = roxmltree::Document::parse(atom)?;

    let entry = match doc.descendants().find(|n| n.tag_name().name() == "entry") {
        Some(e) => e,
        None => return Ok(None),
    };

    let child_text = |tag: &str| {
        entry
            .children()
            .find(|c| c.tag_name().name() == tag)
            .and_then(|n| n.text())
            .map(collapse_ws)
    };

    let title = child_text("title");
    let abstract_text = child_text("summary");
    let url = child_text("id");
    let year =
        child_text("published").and_then(|s| s.get(0..4).and_then(|y| y.parse::<i64>().ok()));

    let authors: Vec<String> = entry
        .children()
        .filter(|c| c.tag_name().name() == "author")
        .filter_map(|a| {
            a.children()
                .find(|n| n.tag_name().name() == "name")
                .and_then(|n| n.text())
                .map(collapse_ws)
        })
        .collect();

    let doi = entry
        .children()
        .find(|c| c.tag_name().name() == "doi")
        .and_then(|n| n.text())
        .map(|s| s.trim().to_string());

    Ok(Some(ResolvedMetadata {
        title,
        abstract_text,
        authors,
        venue: None,
        year,
        doi,
        arxiv_id: None, // stamped by the Resolver, which knows the queried id
        dblp_key: None,
        url,
        source: "arxiv".to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/arxiv_attention.xml"
    ));

    #[test]
    fn parses_arxiv_entry() {
        let md = parse(FIXTURE).unwrap().unwrap();
        assert_eq!(md.title.as_deref(), Some("Attention Is All You Need"));
        assert_eq!(md.year, Some(2017));
        assert_eq!(md.authors, vec!["Ashish Vaswani", "Noam Shazeer"]);
        assert_eq!(md.doi.as_deref(), Some("10.5555/3295222.3295349"));
        assert_eq!(md.url.as_deref(), Some("http://arxiv.org/abs/1706.03762v5"));
        assert!(md
            .abstract_text
            .unwrap()
            .starts_with("The dominant sequence"));
        assert_eq!(md.source, "arxiv");
    }

    #[test]
    fn empty_feed_is_none() {
        let feed = r#"<feed xmlns="http://www.w3.org/2005/Atom"></feed>"#;
        assert!(parse(feed).unwrap().is_none());
    }
}
