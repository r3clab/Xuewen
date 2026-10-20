use anyhow::Result;
use serde_json::Value;

use super::ResolvedMetadata;
use crate::http::HttpClient;
use crate::text::collapse_ws;

/// Search OpenReview notes by title. Returns raw JSON.
///
/// `/notes/search` is the only note endpoint that answers unauthenticated:
/// `/notes?id=…` returns a 403 `ChallengeRequiredError`. It is a fuzzy
/// full-text search over every note in the forum — reviews and comments
/// included — so the caller's similarity gate, not this endpoint, is what
/// makes a hit trustworthy.
pub(crate) async fn search(http: &HttpClient, base: &str, title: &str) -> Result<String> {
    let req = http.get(&format!("{base}/notes/search")).query(&[
        ("term", title),
        ("content", "all"),
        ("group", "all"),
        ("source", "all"),
        ("limit", "5"),
    ]);
    http.send_text(req).await
}

/// Parse a `/notes/search` response into candidate records (possibly empty).
pub(crate) fn parse(json: &str) -> Result<Vec<ResolvedMetadata>> {
    let v: Value = serde_json::from_str(json)?;
    let Some(notes) = v["notes"].as_array() else {
        return Ok(Vec::new());
    };
    Ok(notes.iter().filter_map(parse_note).collect())
}

/// One note, or `None` when it is not a usable publication record.
fn parse_note(n: &Value) -> Option<ResolvedMetadata> {
    let content = &n["content"];
    let title = field(content, "title")
        .and_then(Value::as_str)
        .map(collapse_ws)
        .filter(|s| !s.is_empty())?;

    // A note carrying no venue is a review, a comment or a bare forum post,
    // not a paper — and one whose venue records a non-acceptance would be a
    // lie if stored as this paper's venue. Both drop out so resolution falls
    // through to the next source.
    let venue_raw = field(content, "venue")
        .and_then(Value::as_str)
        .map(collapse_ws)
        .filter(|v| accepted(v))?;
    let (venue, year) = split_venue(&venue_raw);

    let authors = field(content, "authors")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(collapse_ws)
                .collect()
        })
        .unwrap_or_default();

    Some(ResolvedMetadata {
        title: Some(title),
        abstract_text: field(content, "abstract")
            .and_then(Value::as_str)
            .map(collapse_ws)
            .filter(|s| !s.is_empty()),
        authors,
        venue,
        year,
        doi: None,
        arxiv_id: None,
        dblp_key: None,
        // The forum page, not the `pdf` field: OpenReview answers 403 to an
        // unauthenticated PDF fetch, so a direct link would only ever 403.
        url: n["forum"]
            .as_str()
            .or_else(|| n["id"].as_str())
            .map(|id| format!("https://openreview.net/forum?id={id}")),
        source: "openreview".to_string(),
    })
}

/// One content field under either API's shape: API 2 (`api2.openreview.net`,
/// which holds 2023 onwards) wraps every value as `{"value": …}`, while API 1
/// (which still holds ICLR 2015-2022) stores it bare.
fn field<'a>(content: &'a Value, key: &str) -> Option<&'a Value> {
    let v = content.get(key)?;
    Some(v.get("value").unwrap_or(v))
}

/// Venue strings OpenReview gives a submission that was never accepted:
/// "Submitted to ICLR 2026", "ICLR 2022 Submitted", "ICLR 2024 Conference
/// Withdrawn Submission", "ICLR 2026 Conference Desk Rejected Submission".
/// Never the bare word "review" — "Review of Symbolic Logic" is a real journal
/// that OpenReview hosts.
const UNACCEPTED: &[&str] = &["submitted", "withdrawn", "rejected", "under review"];

/// Whole words only, so that a venue reading "Resubmitted" is not condemned by
/// the "submitted" marker it merely contains.
fn accepted(venue: &str) -> bool {
    let v = format!(" {} ", venue.to_lowercase());
    !UNACCEPTED.iter().any(|m| v.contains(&format!(" {m} ")))
}

/// Split OpenReview's single venue string into DBLP's shape — an acronym plus
/// a year. Everything before the four-digit year is the venue, which drops the
/// decision suffix the site appends ("ICLR 2026 Oral", "ICLR 2021 Poster",
/// "ICLR 2023 notable top 25%") without needing to enumerate the suffixes.
fn split_venue(venue: &str) -> (Option<String>, Option<i64>) {
    let words: Vec<&str> = venue.split_whitespace().collect();
    let at = words.iter().position(|w| year_of(w).is_some());
    match at {
        Some(i) => {
            let name = words[..i].join(" ");
            ((!name.is_empty()).then_some(name), year_of(words[i]))
        }
        None => ((!venue.is_empty()).then(|| venue.to_string()), None),
    }
}

fn year_of(w: &str) -> Option<i64> {
    (w.len() == 4 && w.bytes().all(|b| b.is_ascii_digit()))
        .then(|| w.parse::<i64>().ok())
        .flatten()
        .filter(|y| (1900..=2100).contains(y))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/openreview_succinct.json"
    ));

    #[test]
    fn parses_api2_note() {
        let cands = parse(FIXTURE).unwrap();
        // The second note in the real response is a peer review: no title, no
        // venue, and it must not become a candidate.
        assert_eq!(cands.len(), 1);
        let c = &cands[0];
        assert_eq!(
            c.title.as_deref(),
            Some("Transformers are Inherently Succinct")
        );
        assert_eq!(c.venue.as_deref(), Some("ICLR"));
        assert_eq!(c.year, Some(2026));
        assert_eq!(
            c.authors,
            vec![
                "Pascal Bergsträßer",
                "Ryan Cotterell",
                "Anthony Widjaja Lin"
            ]
        );
        assert!(c
            .abstract_text
            .as_deref()
            .unwrap()
            .starts_with("We study succinctness"));
        assert_eq!(
            c.url.as_deref(),
            Some("https://openreview.net/forum?id=Yxz92UuPLQ")
        );
        assert_eq!(c.source, "openreview");
    }

    #[test]
    fn parses_bare_api1_content() {
        // API 1 stores content values unwrapped, without the {"value": …} box.
        let json = r#"{"notes":[{"id":"St1giarCHLP","forum":"St1giarCHLP","content":{
            "title":"Denoising Diffusion Implicit Models",
            "authors":["Jiaming Song","Chenlin Meng","Stefano Ermon"],
            "venue":"ICLR 2021 Poster","venueid":"ICLR.cc/2021/Conference"
        }}]}"#;
        let c = &parse(json).unwrap()[0];
        assert_eq!(
            c.title.as_deref(),
            Some("Denoising Diffusion Implicit Models")
        );
        assert_eq!(c.venue.as_deref(), Some("ICLR"));
        assert_eq!(c.year, Some(2021));
        assert_eq!(c.authors.len(), 3);
    }

    #[test]
    fn drops_notes_that_were_never_accepted() {
        // Every one of these venue strings is a real OpenReview value.
        for venue in [
            "Submitted to ICLR 2026",
            "ICLR 2022 Submitted",
            "ICLR 2024 Conference Withdrawn Submission",
            "ICLR 2026 Conference Desk Rejected Submission",
        ] {
            let json = format!(
                r#"{{"notes":[{{"id":"x","content":{{"title":{{"value":"A Paper"}},"venue":{{"value":"{venue}"}}}}}}]}}"#
            );
            assert!(
                parse(&json).unwrap().is_empty(),
                "expected {venue:?} to be dropped"
            );
        }
    }

    #[test]
    fn a_marker_inside_a_longer_word_does_not_condemn_a_venue() {
        let json = r#"{"notes":[{"id":"x","content":{
            "title":{"value":"A Paper"},"venue":{"value":"Resubmitted Papers Track 2026"}}}]}"#;
        let c = &parse(json).unwrap()[0];
        assert_eq!(c.venue.as_deref(), Some("Resubmitted Papers Track"));
    }

    #[test]
    fn keeps_a_journal_whose_name_contains_review() {
        // "Review of Symbolic Logic" is a real venue: only whole phrases like
        // "under review" mark a non-acceptance, never the bare word.
        let json = r#"{"notes":[{"id":"x","content":{
            "title":{"value":"A Paper"},"venue":{"value":"Review of Symbolic Logic"}}}]}"#;
        let c = &parse(json).unwrap()[0];
        assert_eq!(c.venue.as_deref(), Some("Review of Symbolic Logic"));
        assert_eq!(c.year, None);
    }

    #[test]
    fn splits_decision_suffixes_off_the_venue() {
        assert_eq!(
            split_venue("ICLR 2023 notable top 25%"),
            (Some("ICLR".into()), Some(2023))
        );
        assert_eq!(split_venue("ICML 2022"), (Some("ICML".into()), Some(2022)));
        assert_eq!(split_venue("TMLR"), (Some("TMLR".into()), None));
        assert_eq!(split_venue(""), (None, None));
    }

    #[test]
    fn no_notes_is_empty() {
        assert!(parse(r#"{"notes":[],"count":0}"#).unwrap().is_empty());
        assert!(parse(r#"{"count":0}"#).unwrap().is_empty());
    }
}
