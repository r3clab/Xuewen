use anyhow::{bail, Result};

use crate::http::HttpClient;
use crate::text::collapse_ws;

/// A new arXiv paper parsed from the announcement feed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Candidate {
    /// Versionless id, e.g. "2507.01234".
    pub(crate) arxiv_id: String,
    pub(crate) title: String,
    pub(crate) authors: Vec<String>,
    pub(crate) abstract_text: String,
    pub(crate) categories: Vec<String>,
}

/// GET the announcement feed for `categories`, joined with '+'
/// (rss.arxiv.org serves one combined feed for multiple categories).
pub(crate) async fn fetch_feed(
    http: &HttpClient,
    feed_base: &str,
    categories: &[String],
) -> Result<String> {
    let url = format!(
        "{}/{}",
        feed_base.trim_end_matches('/'),
        categories.join("+")
    );
    http.get_text(&url).await
}

/// "2507.01234v2" -> "2507.01234"; ids without a version pass through.
pub(crate) fn strip_version(id: &str) -> String {
    match id.rfind('v') {
        Some(i) if i + 1 < id.len() && id[i + 1..].chars().all(|c| c.is_ascii_digit()) => {
            id[..i].to_string()
        }
        _ => id.to_string(),
    }
}

/// Parse the rss.arxiv.org Atom feed, keeping `new` announcements (plus
/// `cross` when `include_cross_list`); `replace*` is always dropped.
pub(crate) fn parse_feed(xml: &str, include_cross_list: bool) -> Result<Vec<Candidate>> {
    let doc = roxmltree::Document::parse(xml)?;
    let root = doc.root_element();
    let feed_title = root
        .children()
        .find(|n| n.tag_name().name() == "title")
        .and_then(|n| n.text())
        .unwrap_or("");
    if feed_title.contains("Feed error for query") {
        bail!("arXiv feed error — check [daily].categories: {feed_title}");
    }

    let mut out = Vec::new();
    for entry in root.children().filter(|n| n.tag_name().name() == "entry") {
        let child_text = |tag: &str| {
            entry
                .children()
                .find(|c| c.tag_name().name() == tag)
                .and_then(|n| n.text())
                .map(collapse_ws)
        };

        let announce = child_text("announce_type").unwrap_or_else(|| "new".into());
        let keep = announce == "new" || (include_cross_list && announce == "cross");
        if !keep {
            continue;
        }

        let Some(raw_id) = child_text("id") else {
            continue;
        };
        let arxiv_id = strip_version(raw_id.trim_start_matches("oai:arXiv.org:"));
        let Some(title) = child_text("title") else {
            continue;
        };

        // Summary looks like "arXiv:...v1 Announce Type: new Abstract: <text>".
        let raw_summary = child_text("summary").unwrap_or_default();
        let abstract_text = match raw_summary.find("Abstract:") {
            Some(i) => collapse_ws(&raw_summary[i + "Abstract:".len()..]),
            None => raw_summary,
        };

        // Authors: dc:creator ("A, B"), falling back to <author><name>.
        let mut authors: Vec<String> = entry
            .children()
            .filter(|c| c.tag_name().name() == "creator")
            .filter_map(|n| n.text())
            .flat_map(|t| t.split(", ").map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect();
        if authors.is_empty() {
            authors = entry
                .children()
                .filter(|c| c.tag_name().name() == "author")
                .filter_map(|a| {
                    a.children()
                        .find(|n| n.tag_name().name() == "name")
                        .and_then(|n| n.text())
                        .map(|s| s.trim().to_string())
                })
                .collect();
        }

        let categories: Vec<String> = entry
            .children()
            .filter(|c| c.tag_name().name() == "category")
            .filter_map(|c| c.attribute("term"))
            .map(String::from)
            .collect();

        out.push(Candidate {
            arxiv_id,
            title,
            authors,
            abstract_text,
            categories,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom"
      xmlns:dc="http://purl.org/dc/elements/1.1/"
      xmlns:arxiv="http://arxiv.org/schemas/atom">
  <title>cs.AI updates on arXiv.org</title>
  <entry>
    <id>oai:arXiv.org:2507.00001v2</id>
    <title>Attention Is Still
      All You Need</title>
    <summary>arXiv:2507.00001v2 Announce Type: new
Abstract: We revisit attention
and find it sufficient.</summary>
    <dc:creator>Ada Lovelace, Alan Turing</dc:creator>
    <category term="cs.AI"/>
    <category term="cs.LG"/>
    <arxiv:announce_type>new</arxiv:announce_type>
  </entry>
  <entry>
    <id>oai:arXiv.org:2507.00002v1</id>
    <title>A Cross-Listed Paper</title>
    <summary>arXiv:2507.00002v1 Announce Type: cross
Abstract: Crossing over.</summary>
    <dc:creator>Grace Hopper</dc:creator>
    <category term="cs.AI"/>
    <arxiv:announce_type>cross</arxiv:announce_type>
  </entry>
  <entry>
    <id>oai:arXiv.org:2507.00003v3</id>
    <title>A Replaced Paper</title>
    <summary>arXiv:2507.00003v3 Announce Type: replace
Abstract: New version.</summary>
    <dc:creator>Nobody</dc:creator>
    <category term="cs.AI"/>
    <arxiv:announce_type>replace</arxiv:announce_type>
  </entry>
</feed>"#;

    #[test]
    fn keeps_new_only_by_default() {
        let out = parse_feed(FEED, false).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].arxiv_id, "2507.00001");
    }

    #[test]
    fn include_cross_list_keeps_cross_never_replace() {
        let out = parse_feed(FEED, true).unwrap();
        let ids: Vec<&str> = out.iter().map(|c| c.arxiv_id.as_str()).collect();
        assert_eq!(ids, vec!["2507.00001", "2507.00002"]);
    }

    #[test]
    fn extracts_fields_and_strips_noise() {
        let c = &parse_feed(FEED, false).unwrap()[0];
        assert_eq!(c.title, "Attention Is Still All You Need");
        assert_eq!(
            c.abstract_text,
            "We revisit attention and find it sufficient."
        );
        assert_eq!(c.authors, vec!["Ada Lovelace", "Alan Turing"]);
        assert_eq!(c.categories, vec!["cs.AI", "cs.LG"]);
    }

    #[test]
    fn feed_error_title_is_an_error() {
        let xml = r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Feed error for query: nosuch.CAT</title>
</feed>"#;
        let err = parse_feed(xml, false).unwrap_err().to_string();
        assert!(err.contains("categories"), "got: {err}");
    }

    #[test]
    fn strip_version_handles_old_style_ids() {
        assert_eq!(strip_version("2507.00001v12"), "2507.00001");
        assert_eq!(strip_version("cs/0501001v2"), "cs/0501001");
        assert_eq!(strip_version("2507.00001"), "2507.00001");
    }

    #[tokio::test]
    async fn fetch_feed_joins_categories_with_plus() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/atom/cs.AI+cs.LG"))
            .respond_with(ResponseTemplate::new(200).set_body_string(FEED))
            .expect(1)
            .mount(&server)
            .await;
        let http = crate::http::HttpClient::new(
            reqwest::Client::new(),
            crate::http::RetryPolicy::fast_for_tests(),
        );
        let base = format!("{}/atom", server.uri());
        let xml = fetch_feed(&http, &base, &["cs.AI".into(), "cs.LG".into()])
            .await
            .unwrap();
        assert!(xml.contains("2507.00001"));
    }
}
