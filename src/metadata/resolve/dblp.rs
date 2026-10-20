use anyhow::Result;
use serde_json::Value;

use super::ResolvedMetadata;
use crate::http::HttpClient;
use crate::text::collapse_ws;

/// Search DBLP publications by title. Returns raw JSON.
pub(crate) async fn fetch(http: &HttpClient, base: &str, title: &str) -> Result<String> {
    let req = http.get(&format!("{base}/search/publ/api")).query(&[
        ("q", title),
        ("format", "json"),
        ("h", "5"),
    ]);
    http.send_text(req).await
}

/// Parse a DBLP publ-search response into candidate records (possibly empty).
pub(crate) fn parse(json: &str) -> Result<Vec<ResolvedMetadata>> {
    let v: Value = serde_json::from_str(json)?;
    let hits = values(&v["result"]["hits"]["hit"]);

    let mut out = Vec::new();
    for h in hits {
        let info = &h["info"];
        if info.is_null() {
            continue;
        }
        let title = info["title"].as_str().map(clean_title);
        let year = info["year"].as_str().and_then(|s| s.parse::<i64>().ok());
        let venue = super::venue_with_issue(venue_of(&info["venue"]), info["number"].as_str());
        let doi = info["doi"].as_str().map(str::to_string);
        let url = info["ee"]
            .as_str()
            .or_else(|| info["url"].as_str())
            .map(str::to_string);
        let dblp_key = info["key"].as_str().map(str::to_string);
        let authors = values(&info["authors"]["author"])
            .iter()
            .filter_map(|a| a["text"].as_str().map(clean_author))
            .collect();

        out.push(ResolvedMetadata {
            title,
            abstract_text: None,
            authors,
            venue,
            year,
            doi,
            arxiv_id: None,
            dblp_key,
            url,
            source: "dblp".to_string(),
        });
    }
    Ok(out)
}

/// A DBLP JSON field that may be a single object, an array, or absent.
fn values(v: &Value) -> Vec<&Value> {
    if let Some(arr) = v.as_array() {
        arr.iter().collect()
    } else if v.is_null() {
        Vec::new()
    } else {
        vec![v]
    }
}

/// DBLP `venue` may be a string or an array of strings.
fn venue_of(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        Some(collapse_ws(s))
    } else if let Some(arr) = v.as_array() {
        arr.iter().find_map(|x| x.as_str()).map(collapse_ws)
    } else {
        None
    }
}

/// DBLP titles end with a trailing period; strip it and collapse whitespace.
fn clean_title(s: &str) -> String {
    collapse_ws(s.trim_end_matches('.'))
}

/// DBLP appends a four-digit homonym number to disambiguate authors who share a
/// name (e.g. "Hong Hu 0004"); strip that suffix so we store the plain name.
fn clean_author(s: &str) -> String {
    let name = collapse_ws(s);
    if let Some((head, tail)) = name.rsplit_once(' ') {
        if tail.len() == 4 && tail.bytes().all(|b| b.is_ascii_digit()) {
            return head.to_string();
        }
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/dblp_kgat.json"
    ));

    #[test]
    fn parses_dblp_hit() {
        let cands = parse(FIXTURE).unwrap();
        assert_eq!(cands.len(), 1);
        let c = &cands[0];
        assert_eq!(
            c.title.as_deref(),
            Some("KGAT: Knowledge Graph Attention Network for Recommendation")
        );
        assert_eq!(c.year, Some(2019));
        assert_eq!(c.venue.as_deref(), Some("KDD"));
        assert_eq!(c.doi.as_deref(), Some("10.1145/3292500.3330701"));
        assert_eq!(c.dblp_key.as_deref(), Some("conf/kdd/WangHCLC19"));
        assert_eq!(c.authors, vec!["Xiang Wang", "Xiangnan He", "Yixin Cao"]);
        assert_eq!(c.source, "dblp");
    }

    #[test]
    fn a_pacmpl_hit_carries_its_conference_issue() {
        let json = r#"{"result":{"hits":{"@total":"1","hit":[{"info":{
            "title":"Incorrectness logic.","year":"2020",
            "venue":"Proc. ACM Program. Lang.","number":"POPL",
            "doi":"10.1145/3371078","key":"journals/pacmpl/OHearn20"}}]}}}"#;
        let c = &parse(json).unwrap()[0];
        assert_eq!(c.venue.as_deref(), Some("Proc. ACM Program. Lang. (POPL)"));
    }

    #[test]
    fn a_numeric_issue_is_not_a_venue_name() {
        let json = r#"{"result":{"hits":{"@total":"1","hit":[{"info":{
            "title":"A paper.","year":"2020","venue":"Artif. Intell.","number":"3"}}]}}}"#;
        let c = &parse(json).unwrap()[0];
        assert_eq!(c.venue.as_deref(), Some("Artif. Intell."));
    }

    #[test]
    fn zero_hits_is_empty() {
        let json = r#"{"result":{"hits":{"@total":"0"}}}"#;
        assert!(parse(json).unwrap().is_empty());
    }

    #[test]
    fn strips_dblp_homonym_numbers() {
        // DBLP appends a four-digit homonym number to disambiguate authors who
        // share a name; it must not end up in the stored author list.
        let json = r#"{"result":{"hits":{"hit":[{"info":{
            "title":"A Disambiguated Paper.","year":"2021",
            "authors":{"author":[
                {"@pid":"1","text":"Jinho Jung 0001"},
                {"@pid":"2","text":"Hong Hu 0004"},
                {"@pid":"3","text":"Xiang Wang"}
            ]}
        }}]}}}"#;
        let cands = parse(json).unwrap();
        assert_eq!(
            cands[0].authors,
            vec!["Jinho Jung", "Hong Hu", "Xiang Wang"]
        );
    }

    #[test]
    fn single_author_object_is_handled() {
        // DBLP returns a bare object (not array) when there is exactly one author.
        let json = r#"{"result":{"hits":{"hit":[{"info":{
            "title":"A One Author Paper.","year":"2020",
            "authors":{"author":{"@pid":"1","text":"Solo Writer"}}
        }}]}}}"#;
        let cands = parse(json).unwrap();
        assert_eq!(cands[0].authors, vec!["Solo Writer"]);
    }
}
