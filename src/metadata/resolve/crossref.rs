use anyhow::Result;
use serde_json::Value;

use super::ResolvedMetadata;
use crate::http::{encode_doi_path, HttpClient};
use crate::text::{collapse_ws, strip_tags};

/// Fetch the Crossref work record for a DOI from `{base}/works/{doi}`.
pub(crate) async fn fetch(http: &HttpClient, base: &str, doi: &str) -> Result<String> {
    // Crossref requires the DOI URL-encoded: a raw `#`/`?` (legal in DOIs)
    // would truncate the request path.
    let url = format!("{base}/works/{}", encode_doi_path(doi));
    http.get_text(&url).await
}

/// Search Crossref by bibliographic string (title). Returns raw JSON.
pub(crate) async fn search(http: &HttpClient, base: &str, title: &str) -> Result<String> {
    let req = http
        .get(&format!("{base}/works"))
        .query(&[("query.bibliographic", title), ("rows", "5")]);
    http.send_text(req).await
}

/// Parse a Crossref `/works/{doi}` JSON body. Returns `Ok(None)` if there is no message.
pub(crate) fn parse(json: &str) -> Result<Option<ResolvedMetadata>> {
    let v: Value = serde_json::from_str(json)?;
    let m = &v["message"];
    if m.is_null() {
        return Ok(None);
    }
    Ok(Some(parse_item(m)))
}

/// Parse a Crossref `/works?query...` search body into candidate records.
pub(crate) fn parse_search(json: &str) -> Result<Vec<ResolvedMetadata>> {
    let v: Value = serde_json::from_str(json)?;
    let items = v["message"]["items"].as_array();
    Ok(items
        .map(|arr| arr.iter().map(parse_item).collect())
        .unwrap_or_default())
}

/// Extract normalized metadata from a single Crossref work object
/// (either `message` for a DOI lookup or one element of `message.items`).
fn parse_item(m: &Value) -> ResolvedMetadata {
    let title = m["title"].get(0).and_then(Value::as_str).map(collapse_ws);
    let venue = super::venue_with_issue(
        m["container-title"]
            .get(0)
            .and_then(Value::as_str)
            .map(collapse_ws),
        m["issue"].as_str(),
    );
    let doi = m["DOI"].as_str().map(str::to_string);
    let url = m["URL"].as_str().map(str::to_string);
    let abstract_text = m["abstract"].as_str().map(strip_tags);
    let year = m["issued"]["date-parts"]
        .get(0)
        .and_then(|dp| dp.get(0))
        .and_then(Value::as_i64);

    let authors: Vec<String> = m["author"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    let given = a["given"].as_str().unwrap_or("");
                    let family = a["family"].as_str().unwrap_or("");
                    let name = format!("{given} {family}").trim().to_string();
                    (!name.is_empty()).then_some(name)
                })
                .collect()
        })
        .unwrap_or_default();

    ResolvedMetadata {
        title,
        abstract_text,
        authors,
        venue,
        year,
        doi,
        arxiv_id: None,
        dblp_key: None,
        url,
        source: "crossref".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/crossref_kgat.json"
    ));
    const SEARCH_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/crossref_search_kgat.json"
    ));

    #[test]
    fn parses_crossref_work() {
        let md = parse(FIXTURE).unwrap().unwrap();
        assert_eq!(
            md.title.as_deref(),
            Some("KGAT: Knowledge Graph Attention Network for Recommendation")
        );
        assert_eq!(md.year, Some(2019));
        assert_eq!(md.authors, vec!["Xiang Wang", "Xiangnan He"]);
        assert_eq!(md.doi.as_deref(), Some("10.1145/3292500.3330701"));
        assert_eq!(
            md.abstract_text.as_deref(),
            Some("Knowledge graphs are used to improve recommendation.")
        );
        assert!(md
            .venue
            .unwrap()
            .starts_with("Proceedings of the 25th ACM SIGKDD"));
        assert_eq!(md.source, "crossref");
    }

    #[test]
    fn pacmpl_issue_names_the_conference_in_the_venue() {
        // PACMPL is one journal per conference issue: without the issue,
        // "Incorrectness logic" reads as the container, not as POPL.
        let json = r#"{"status":"ok","message":{"title":["Incorrectness logic"],
            "container-title":["Proceedings of the ACM on Programming Languages"],
            "issue":"POPL","DOI":"10.1145/3371078","issued":{"date-parts":[[2019,12]]}}}"#;
        let md = parse(json).unwrap().unwrap();
        assert_eq!(
            md.venue.as_deref(),
            Some("Proceedings of the ACM on Programming Languages (POPL)")
        );
    }

    #[test]
    fn a_numeric_issue_names_nothing_and_is_left_off() {
        let json = r#"{"status":"ok","message":{"title":["A paper"],
            "container-title":["Artificial Intelligence"],"issue":"3",
            "DOI":"10.1/x","issued":{"date-parts":[[2020]]}}}"#;
        let md = parse(json).unwrap().unwrap();
        assert_eq!(md.venue.as_deref(), Some("Artificial Intelligence"));
    }

    #[test]
    fn missing_message_is_none() {
        assert!(parse(r#"{"status":"ok"}"#).unwrap().is_none());
    }

    #[test]
    fn parses_crossref_search_items() {
        let cands = parse_search(SEARCH_FIXTURE).unwrap();
        assert_eq!(cands.len(), 1);
        assert_eq!(
            cands[0].title.as_deref(),
            Some("KGAT: Knowledge Graph Attention Network for Recommendation")
        );
        assert_eq!(cands[0].doi.as_deref(), Some("10.1145/3292500.3330701"));
        assert_eq!(cands[0].year, Some(2019));
    }

    #[test]
    fn empty_search_is_empty() {
        assert!(parse_search(r#"{"message":{"items":[]}}"#)
            .unwrap()
            .is_empty());
    }
}
