//! Pattern-matching bibliography parser: the free path in front of the LLM.
//!
//! One bibliography = one style (the camera-ready template fixes the
//! BibTeX style), so the style is detected once per paper by voting across
//! entries, then every entry is parsed with that style's grammar and
//! strictly validated — a false None costs one LLM entry; a false Some
//! shows a wrong popover.

use regex::Regex;
use std::sync::LazyLock;

use super::StructuredReference;

/// `[12]` / `12.` entry markers left on entries by the frontend extraction.
/// The `12.` form requires trailing whitespace so "1.5 Gbps…" survives.
static MARKER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\[\d{1,3}\]\s*|\d{1,3}\.\s+)").unwrap());

pub(super) fn strip_marker(s: &str) -> &str {
    let t = s.trim();
    match MARKER_RE.find(t) {
        Some(m) => t[m.end()..].trim_start(),
        None => t,
    }
}

/// Style-independent fields extractable from any entry.
pub(super) struct UniversalFields {
    year: Option<i64>,
    doi: Option<String>,
    arxiv_id: Option<String>,
    url: Option<String>,
}

static URL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https?://[^\s<>]+").unwrap());
static DOI_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b10\.\d{4,9}/[^\s<>]+").unwrap());
static ARXIV_NEW_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(\d{4}\.\d{4,5})(?:v\d+)?\b").unwrap());
static ARXIV_OLD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b([a-z-]+(?:\.[A-Z]{2})?/\d{7})\b").unwrap());
static YEAR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:19|20)\d{2}\b").unwrap());
static RANGE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+\s*[-–—]\s*\d+").unwrap());

pub(super) fn universal_fields(entry: &str) -> UniversalFields {
    let trim_trail = |s: &str| s.trim_end_matches(['.', ',', ';', ')']).to_string();
    let url = URL_RE.find(entry).map(|m| trim_trail(m.as_str()));
    let doi = DOI_RE.find(entry).map(|m| trim_trail(m.as_str()));
    // arXiv entries always say "arXiv"; a bare NNNN.NNNNN elsewhere is not
    // evidence enough (DOI suffixes and decimals produce lookalikes).
    let arxiv_id = if entry.to_lowercase().contains("arxiv") {
        ARXIV_NEW_RE
            .captures(entry)
            .map(|c| c[1].to_string())
            .or_else(|| ARXIV_OLD_RE.captures(entry).map(|c| c[1].to_string()))
    } else {
        None
    };
    // Year: search a copy with URLs/DOIs/arXiv-ids/number-ranges blanked
    // (page ranges like 1993–2008 read as years) and keep the LAST in-range
    // hit — years sit at the entry tail in every style whose grammar doesn't
    // extract its own (IEEE, plain).
    let mut cleaned = entry.to_string();
    for re in [&*URL_RE, &*DOI_RE, &*ARXIV_NEW_RE, &*RANGE_RE] {
        cleaned = re.replace_all(&cleaned, " ").into_owned();
    }
    let year = YEAR_RE
        .find_iter(&cleaned)
        .filter_map(|m| m.as_str().parse::<i64>().ok())
        .filter(|y| (1900..=2035).contains(y))
        .last();
    UniversalFields {
        year,
        doi,
        arxiv_id,
        url,
    }
}

/// Word endings that do NOT close a sentence even before ". ".
// "al" is intentionally absent: the period after "et al." genuinely ends an author segment.
const ABBREVS: &[&str] = &[
    "proc", "conf", "symp", "int", "intl", "trans", "vol", "no", "pp", "ed", "eds", "univ", "dept",
    "rev", "jr", "sr", "st",
];

/// "J." (initials, incl. "D.P"), "(J" and known abbreviations don't end a sentence.
fn is_initial_or_abbrev(word: &str) -> bool {
    let w = word.trim_start_matches(['(', '[', '"', '\u{201C}', '\u{201D}', '\'']);
    let alpha = w.chars().filter(|c| c.is_alphabetic()).count();
    if alpha == 0 {
        return false;
    }
    let all_upper = w
        .chars()
        .filter(|c| c.is_alphabetic())
        .all(|c| c.is_uppercase());
    if all_upper && alpha <= 3 && w.chars().count() <= 5 {
        return true;
    }
    ABBREVS.contains(&w.trim_end_matches('.').to_ascii_lowercase().as_str())
}

/// Split at ". " boundaries that actually end a sentence.
pub(super) fn split_sentences(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (i, c) in s.char_indices() {
        if c != '.' {
            continue;
        }
        if !s[i + 1..].chars().next().is_none_or(|c| c.is_whitespace()) {
            continue; // mid-token period: DOI, URL, "1.5"
        }
        let prev = s[start..i].split_whitespace().last().unwrap_or("");
        if is_initial_or_abbrev(prev) {
            continue;
        }
        let seg = s[start..i].trim();
        if !seg.is_empty() {
            out.push(seg.to_string());
        }
        start = i + 1;
    }
    let tail = s[start..].trim().trim_end_matches('.').trim();
    if !tail.is_empty() {
        out.push(tail.to_string());
    }
    out
}

/// "A and B", "A, B, and C", "A & B" → individual names; "et al." dropped.
pub(super) fn split_authors(s: &str) -> Vec<String> {
    s.replace(" and ", ", ")
        .replace(" & ", ", ")
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty() && !t.trim_end_matches('.').eq_ignore_ascii_case("et al"))
        .map(String::from)
        .collect()
}

/// Strict name shape: uppercase present, sane length, no digits/parens.
pub(super) fn looks_like_name(a: &str) -> bool {
    let n = a.chars().count();
    (2..=60).contains(&n)
        && a.chars().any(|c| c.is_uppercase())
        && !a
            .chars()
            .any(|c| c.is_ascii_digit() || c == '(' || c == ')')
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Style {
    Ieee,
    Acm,
    Lncs,
    Plain,
}

impl Style {
    /// Every variant, so `detect_style` can tally votes without an
    /// enum-as-index invariant. A variant missing here panics the vote
    /// (`position(..).expect`) the first time that style is seen — which the
    /// per-style detection tests do on every run.
    pub(super) const ALL: [Style; 4] = [Style::Ieee, Style::Acm, Style::Lncs, Style::Plain];
}

/// IEEE titles are quoted (straight or curly).
pub(super) static QUOTED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"["\u{201C}]([^"\u{201C}\u{201D}]{4,}?)["\u{201D}]"#).unwrap());
/// ACM: leading author segment, then ". YYYY. ", then the rest.
pub(super) static ACM_HEAD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)^(.{3,300}?)\.\s+((19|20)\d{2})\.\s+(.+)$").unwrap());
/// LNCS entries start "Lastname, F." — full signature also needs the ".:"
/// that closes the author block.
static LNCS_START_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\p{Lu}[\p{L}'\u{2019}-]+,\s*\p{Lu}\.").unwrap());

fn style_of_entry(e: &str) -> Style {
    if QUOTED_RE.is_match(e) {
        Style::Ieee
    } else if LNCS_START_RE.is_match(e) && e.contains(".:") {
        Style::Lncs
    } else if ACM_HEAD_RE.is_match(e) {
        Style::Acm
    } else {
        Style::Plain
    }
}

/// Publisher family inferred from the paper's venue string — a prior, used
/// only when the content vote is ambiguous. Deliberately not a maintained
/// venue map: four substring rules cover the publisher families.
fn venue_family_style(venue: &str) -> Option<Style> {
    let v = venue.to_ascii_lowercase();
    if v.contains("acm") {
        Some(Style::Acm)
    } else if v.contains("ieee") {
        Some(Style::Ieee)
    } else if v.contains("usenix") {
        Some(Style::Plain)
    } else if v.contains("springer") || v.contains("lncs") || v.contains("lecture notes") {
        Some(Style::Lncs)
    } else {
        None
    }
}

/// One bibliography = one style: vote across entries, ≥60% wins; otherwise
/// the venue family decides; otherwise None (every entry goes to the LLM).
pub(super) fn detect_style(entries: &[&str], venue: Option<&str>) -> Option<Style> {
    if entries.is_empty() {
        return None;
    }
    let mut counts = [0usize; Style::ALL.len()];
    for e in entries {
        let style = style_of_entry(e);
        counts[Style::ALL
            .iter()
            .position(|&s| s == style)
            .expect("Style::ALL lists every variant")] += 1;
    }
    let (best, &max) = counts.iter().enumerate().max_by_key(|(_, c)| **c).unwrap();
    if max * 10 >= entries.len() * 6 {
        return Some(Style::ALL[best]);
    }
    venue.and_then(venue_family_style)
}

/// Venue tail noise: ", vol …", ", pp …", ", 2015", ", doi …".
static VENUE_CUT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i),\s*(vol\b|no\.|pp\.|p\.|pages\b|doi\b|(19|20)\d{2}\b)").unwrap()
});

/// Trim a venue fragment: leading "In "/"In: ", trailing noise, punctuation.
fn clean_venue(v: &str) -> Option<String> {
    let v = v.trim();
    let v = v
        .strip_prefix("In: ")
        .or_else(|| v.strip_prefix("in: "))
        .unwrap_or(v);
    let v = v
        .strip_prefix("In ")
        .or_else(|| v.strip_prefix("in "))
        .unwrap_or(v);
    let cut = VENUE_CUT_RE.find(v).map_or(v.len(), |m| m.start());
    let v = v[..cut].trim().trim_end_matches([',', '.']).trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// IEEE: `A. Author and B. Author, "Title," in Venue, year.` The quoted
/// title is the anchor; year comes from the universal pass.
pub(super) fn parse_ieee(entry: &str) -> StructuredReference {
    let Some(c) = QUOTED_RE.captures(entry) else {
        return StructuredReference::default();
    };
    let m = c.get(0).unwrap();
    let title = c[1].trim().trim_end_matches([',', '.']).trim().to_string();
    let authors_part = entry[..m.start()].trim().trim_end_matches(',').trim();
    let rest = entry[m.end()..].trim_start_matches([',', '.', ' ']);
    StructuredReference {
        authors: split_authors(authors_part),
        title: Some(title),
        venue: clean_venue(rest),
        ..Default::default()
    }
}

/// ACM: `Authors. YYYY. Title. In Venue. Publisher, pages.` The ". YYYY. "
/// anchor separates authors from the rest; title is the first sentence.
pub(super) fn parse_acm(entry: &str) -> StructuredReference {
    let Some(c) = ACM_HEAD_RE.captures(entry) else {
        return StructuredReference::default();
    };
    let year = c[2].parse::<i64>().ok();
    let segs = split_sentences(&c[4]);
    StructuredReference {
        authors: split_authors(c[1].trim()),
        title: segs.first().map(|t| t.trim().to_string()),
        venue: segs.get(1).and_then(|v| clean_venue(v)),
        year,
        ..Default::default()
    }
}

static PAREN_YEAR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\(((19|20)\d{2})\)").unwrap());

/// LNCS: `Last, F., Last, F.: Title. In: Venue, pp. x-y. Publisher (YYYY)`.
/// The ".:" closing the author block is the anchor.
pub(super) fn parse_lncs(entry: &str) -> StructuredReference {
    let Some(pos) = entry.find(".:") else {
        return StructuredReference::default();
    };
    // "Kingma, D.P., Ba, J" splits on ',' into surname/initial tokens;
    // consecutive pairs re-join into "Kingma, D.P.".
    let authors: Vec<String> = entry[..pos + 1]
        .split(',')
        .map(str::trim)
        .collect::<Vec<_>>()
        .chunks(2)
        .map(|pair| pair.join(", "))
        .collect();
    let rest = entry[pos + 2..].trim();
    let segs = split_sentences(rest);
    let venue = rest
        .find("In:")
        .map(|i| &rest[i + 3..])
        .and_then(clean_venue);
    let year = PAREN_YEAR_RE
        .captures_iter(rest)
        .last()
        .and_then(|c| c[1].parse::<i64>().ok());
    StructuredReference {
        authors,
        title: segs.first().map(|t| t.trim().to_string()),
        venue,
        year,
        ..Default::default()
    }
}

/// plain.bst: `Authors. Title. Venue, year.` - period-delimited, no anchors,
/// so this grammar only ever runs when the vote picked it for the whole
/// bibliography, and strict validation guards the output.
pub(super) fn parse_plain(entry: &str) -> StructuredReference {
    let segs = split_sentences(entry);
    if segs.len() < 2 {
        return StructuredReference::default();
    }
    StructuredReference {
        authors: split_authors(&segs[0]),
        title: Some(segs[1].trim().to_string()),
        venue: segs.get(2).and_then(|v| clean_venue(v)),
        ..Default::default()
    }
}

/// Grammar fields win; universal fields fill the gaps.
fn merge_universal(mut r: StructuredReference, u: UniversalFields) -> StructuredReference {
    r.year = r.year.or(u.year);
    r.doi = r.doi.or(u.doi);
    r.arxiv_id = r.arxiv_id.or(u.arxiv_id);
    r.url = r.url.or(u.url);
    r
}

/// Strict per-entry gate: a false None costs one LLM entry; a false Some
/// shows a wrong popover.
fn validate(r: &StructuredReference) -> bool {
    let Some(t) = r.title.as_deref() else {
        return false;
    };
    let n = t.chars().count();
    if !(4..=300).contains(&n) || !t.chars().any(|c| c.is_alphabetic()) || t.contains("http") {
        return false;
    }
    if r.authors.is_empty() || !r.authors.iter().all(|a| looks_like_name(a)) {
        return false;
    }
    if r.year.is_none() && r.doi.is_none() && r.arxiv_id.is_none() {
        return false;
    }
    r.venue.as_deref() != Some(t)
}

/// Parse every entry with the bibliography's detected style. Entries that
/// fail strict validation come back None - the caller's LLM leftovers.
/// Index-aligned with `refs`.
pub(crate) fn parse_all(refs: &[String], venue: Option<&str>) -> Vec<Option<StructuredReference>> {
    let stripped: Vec<&str> = refs.iter().map(|r| strip_marker(r)).collect();
    let Some(style) = detect_style(&stripped, venue) else {
        return vec![None; refs.len()];
    };
    stripped
        .iter()
        .map(|e| {
            let parsed = match style {
                Style::Ieee => parse_ieee(e),
                Style::Acm => parse_acm(e),
                Style::Lncs => parse_lncs(e),
                Style::Plain => parse_plain(e),
            };
            let merged = merge_universal(parsed, universal_fields(e));
            validate(&merged).then_some(merged)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_bracket_and_dot_markers() {
        assert_eq!(strip_marker("[12] D. Kingma"), "D. Kingma");
        assert_eq!(strip_marker("  3.  Smith, J."), "Smith, J.");
        assert_eq!(strip_marker("D. Kingma"), "D. Kingma"); // "D." is not a marker
        assert_eq!(strip_marker("1.5 Gbps links"), "1.5 Gbps links");
        assert_eq!(strip_marker("[0, 1] interval"), "[0, 1] interval");
    }

    #[test]
    fn extracts_universal_fields() {
        let u = universal_fields(
            "D. Kingma and J. Ba, \"Adam,\" arXiv preprint arXiv:1412.6980v9, 2015. \
             https://doi.org/10.48550/arXiv.1412.6980",
        );
        assert_eq!(u.year, Some(2015));
        assert_eq!(u.arxiv_id.as_deref(), Some("1412.6980"));
        assert_eq!(u.doi.as_deref(), Some("10.48550/arXiv.1412.6980"));
        assert_eq!(
            u.url.as_deref(),
            Some("https://doi.org/10.48550/arXiv.1412.6980")
        );
    }

    #[test]
    fn year_ignores_page_ranges_and_ids() {
        // Page range 1993–2008 must not read as a year; real year is 2020.
        let u = universal_fields("A. Author. Title. In CCS, 2020, pp. 1993–2008.");
        assert_eq!(u.year, Some(2020));
        // No "arxiv" marker → NNNN.NNNNN is not an arXiv id, nor a year.
        let u2 = universal_fields("A. Author. Title with 2004.12345 constant. VLDB, 2019.");
        assert_eq!(u2.arxiv_id, None);
        assert_eq!(u2.year, Some(2019));
        // Old-style id.
        let u3 = universal_fields("S. Aaronson. Limits. arXiv:quant-ph/0502072, 2005.");
        assert_eq!(u3.arxiv_id.as_deref(), Some("quant-ph/0502072"));
    }

    #[test]
    fn universal_fields_absent_when_absent() {
        let u = universal_fields("garbage entry with no fields");
        assert!(u.year.is_none() && u.doi.is_none() && u.arxiv_id.is_none() && u.url.is_none());
    }

    #[test]
    fn split_sentences_respects_initials_and_abbrevs() {
        // "D." and "J." are initials, "Ba." ends the author sentence.
        assert_eq!(
            split_sentences("D. P. Kingma and J. Ba. Adam: A method. ICLR, 2015"),
            vec!["D. P. Kingma and J. Ba", "Adam: A method", "ICLR, 2015"]
        );
        // "Proc." and "vol." don't split; "et al." does — it genuinely ends the author list.
        assert_eq!(
            split_sentences("J. Smith et al. Title here. In Proc. of CCS, vol. 3, 2020"),
            vec![
                "J. Smith et al",
                "Title here",
                "In Proc. of CCS, vol. 3, 2020"
            ]
        );
        // Periods not followed by whitespace (DOIs, URLs) never split.
        assert_eq!(
            split_sentences("See 10.1145/1234.5678 now"),
            vec!["See 10.1145/1234.5678 now"]
        );
    }

    #[test]
    fn curly_quote_before_abbrev_does_not_split() {
        // A leading curly quote before an abbreviation must not cause a split.
        assert_eq!(
            split_sentences("See \u{201C}Proc. of CCS\u{201D} for details. Second sentence"),
            vec![
                "See \u{201C}Proc. of CCS\u{201D} for details",
                "Second sentence"
            ]
        );
        // Same for a straight double quote.
        assert_eq!(
            split_sentences("See \"Proc. of CCS\" for details. Second sentence"),
            vec!["See \"Proc. of CCS\" for details", "Second sentence"]
        );
    }

    #[test]
    fn split_authors_handles_and_ampersand_etal() {
        assert_eq!(
            split_authors("D. Kingma and J. Ba"),
            vec!["D. Kingma", "J. Ba"]
        );
        assert_eq!(
            split_authors("Martín Abadi, Andy Chu, and Li Zhang"),
            vec!["Martín Abadi", "Andy Chu", "Li Zhang"]
        );
        assert_eq!(split_authors("A. One & B. Two"), vec!["A. One", "B. Two"]);
        assert_eq!(split_authors("J. Smith, et al."), vec!["J. Smith"]);
    }

    #[test]
    fn name_shape_rejects_junk() {
        assert!(looks_like_name("D. Kingma"));
        assert!(looks_like_name("Martín Abadi"));
        assert!(looks_like_name("KINGMA, D")); // small-caps extraction
        assert!(!looks_like_name("J. (2015)")); // natbib year glued to initial
        assert!(!looks_like_name("x"));
        assert!(!looks_like_name("3rd Workshop"));
        assert!(!looks_like_name("proceedings of the acm")); // no uppercase
    }

    #[test]
    fn detects_style_by_majority_vote() {
        let ieee = vec![
            r#"K. Kim and T. Kim, "PGFUZZ: Policy-guided fuzzing," in Proc. of NDSS, 2021."#,
            r#"D. Kingma and J. Ba, "Adam: A method for stochastic optimization," in Proc. of ICLR, 2015."#,
            "garbled entry without a quote 2020",
        ];
        assert_eq!(detect_style(&ieee, None), Some(Style::Ieee)); // 2/3 ≥ 60%

        let acm = vec![
            "Martín Abadi and Andy Chu. 2016. Deep Learning with Differential Privacy. In CCS.",
            "Jane Doe. 2019. Another Paper Title. In SOSP.",
        ];
        assert_eq!(detect_style(&acm, None), Some(Style::Acm));

        let lncs = vec![
            "Ateniese, G., Magri, B.: Redactable blockchain. In: EuroS&P, pp. 111–126. IEEE (2017)",
            "Kingma, D., Ba, J.: Adam. In: ICLR (2015)",
        ];
        assert_eq!(detect_style(&lncs, None), Some(Style::Lncs));

        let plain = vec![
            "D. Kingma and J. Ba. Adam: A method. ICLR, 2015.",
            "J. Smith. A systems paper. In Proc. of OSDI, 2020.",
        ];
        assert_eq!(detect_style(&plain, None), Some(Style::Plain));
    }

    #[test]
    fn ambiguous_vote_falls_back_to_venue_family() {
        // 1 IEEE + 1 ACM + 1 LNCS: no style reaches 60%.
        let mixed = vec![
            r#"A. B, "Quoted title here," in Proc. X, 2020."#,
            "Jane Doe. 2019. Some Title. In SOSP.",
            "Roe, R.: Colon Title. In: S&P (2018)",
        ];
        assert_eq!(detect_style(&mixed, None), None);
        assert_eq!(
            detect_style(
                &mixed,
                Some("2021 IEEE Symposium on Security and Privacy (SP)")
            ),
            Some(Style::Ieee)
        );
        assert_eq!(
            detect_style(&mixed, Some("Proceedings of the ACM SIGSAC CCS")),
            Some(Style::Acm)
        );
        assert_eq!(
            detect_style(&mixed, Some("USENIX Security Symposium")),
            Some(Style::Plain)
        );
        assert_eq!(
            detect_style(&mixed, Some("ESORICS, Lecture Notes in CS")),
            Some(Style::Lncs)
        );
        assert_eq!(detect_style(&mixed, Some("Journal of Cryptology")), None);
    }

    #[test]
    fn empty_entries_detect_nothing() {
        assert_eq!(detect_style(&[], None), None);
        assert_eq!(detect_style(&[], Some("ACM CCS")), None);
    }

    #[test]
    fn style_signatures_accept_curly_punctuation() {
        // Curly-quoted IEEE title (U+201C/U+201D) must vote Ieee.
        let e1 = "A. Author, \u{201C}A curly quoted title,\u{201D} in Proc. X, 2020.";
        assert_eq!(detect_style(&[e1], None), Some(Style::Ieee));
        // Curly apostrophe (U+2019) in an LNCS lead surname must vote Lncs.
        let e2 = "O\u{2019}Brien, K.: Some title here. In: ESORICS (2019)";
        assert_eq!(detect_style(&[e2], None), Some(Style::Lncs));
    }

    #[test]
    fn parses_ieee_entries() {
        let r = parse_ieee(
            r#"K. Kim, T. Kim, and E. Fernandez, "PGFUZZ: Policy-guided fuzzing for robotic vehicles," in Proceedings of the Network and Distributed System Security Symposium (NDSS), 2021."#,
        );
        assert_eq!(
            r.title.as_deref(),
            Some("PGFUZZ: Policy-guided fuzzing for robotic vehicles")
        );
        assert_eq!(r.authors, vec!["K. Kim", "T. Kim", "E. Fernandez"]);
        assert_eq!(
            r.venue.as_deref(),
            Some("Proceedings of the Network and Distributed System Security Symposium (NDSS)")
        );

        // Journal form: venue cut before ", vol."; curly quotes.
        let j = parse_ieee(
            "A. Author, \u{201C}A journal paper title,\u{201D} IEEE Transactions on Software Engineering, vol. 47, no. 3, pp. 1\u{2013}20, 2021.",
        );
        assert_eq!(j.title.as_deref(), Some("A journal paper title"));
        assert_eq!(
            j.venue.as_deref(),
            Some("IEEE Transactions on Software Engineering")
        );
    }

    #[test]
    fn ieee_without_quotes_yields_default() {
        assert_eq!(
            parse_ieee("No quotes here at all, 2020."),
            StructuredReference::default()
        );
    }

    #[test]
    fn parses_acm_entries() {
        let r = parse_acm(
            "Mart\u{ED}n Abadi, Andy Chu, Ian Goodfellow, H. Brendan McMahan, Ilya Mironov, Kunal Talwar, and Li Zhang. 2016. Deep Learning with Differential Privacy. In Proceedings of the 2016 ACM SIGSAC Conference on Computer and Communications Security (CCS '16). ACM, 308\u{2013}318.",
        );
        assert_eq!(
            r.title.as_deref(),
            Some("Deep Learning with Differential Privacy")
        );
        assert_eq!(r.year, Some(2016));
        assert_eq!(
            r.authors.first().map(String::as_str),
            Some("Mart\u{ED}n Abadi")
        );
        assert_eq!(r.authors.last().map(String::as_str), Some("Li Zhang"));
        assert_eq!(
            r.venue.as_deref(),
            Some("Proceedings of the 2016 ACM SIGSAC Conference on Computer and Communications Security (CCS '16)")
        );
    }

    #[test]
    fn acm_without_year_anchor_yields_default() {
        assert_eq!(
            parse_acm("D. Kingma and J. Ba. Adam: A method. ICLR, 2015."),
            StructuredReference::default()
        );
    }

    #[test]
    fn parses_lncs_entries() {
        let r = parse_lncs(
            "Ateniese, G., Magri, B., Venturi, D., Andrade, E.: Redactable blockchain \u{2013} or \u{2013} rewriting history in bitcoin and friends. In: 2017 IEEE European Symposium on Security and Privacy (EuroS&P), pp. 111\u{2013}126. IEEE (2017)",
        );
        assert_eq!(
            r.title.as_deref(),
            Some("Redactable blockchain \u{2013} or \u{2013} rewriting history in bitcoin and friends")
        );
        assert_eq!(
            r.authors,
            vec!["Ateniese, G.", "Magri, B.", "Venturi, D.", "Andrade, E."]
        );
        assert_eq!(
            r.venue.as_deref(),
            Some("2017 IEEE European Symposium on Security and Privacy (EuroS&P)")
        );
        assert_eq!(r.year, Some(2017));
    }

    #[test]
    fn lncs_without_colon_yields_default() {
        assert_eq!(
            parse_lncs("D. Kingma and J. Ba. Adam. ICLR, 2015."),
            StructuredReference::default()
        );
    }

    #[test]
    fn parses_plain_entries() {
        let r = parse_plain(
            "D. P. Kingma and J. Ba. Adam: A method for stochastic optimization. ICLR, 2015.",
        );
        assert_eq!(
            r.title.as_deref(),
            Some("Adam: A method for stochastic optimization")
        );
        assert_eq!(r.authors, vec!["D. P. Kingma", "J. Ba"]);
        assert_eq!(r.venue.as_deref(), Some("ICLR"));

        let p = parse_plain("J. Smith. A systems paper. In Proc. of OSDI, 2020.");
        assert_eq!(p.venue.as_deref(), Some("Proc. of OSDI"));
    }

    #[test]
    fn plain_single_segment_yields_default() {
        assert_eq!(parse_plain("garbage"), StructuredReference::default());
    }

    #[test]
    fn parse_all_parses_a_full_ieee_bibliography() {
        let refs: Vec<String> = vec![
            r#"[1] K. Kim, T. Kim, and E. Fernandez, "PGFUZZ: Policy-guided fuzzing for robotic vehicles," in Proceedings of NDSS, 2021."#.into(),
            "[2] D. Kingma and J. Ba, \"Adam: A method for stochastic optimization,\" in Proc. of ICLR, 2015, pp. 1\u{2013}15."
                .into(),
            "[3] garbled fragment no quote".into(),
        ];
        let out = parse_all(&refs, None);
        assert_eq!(out.len(), 3);
        let r0 = out[0].as_ref().unwrap();
        assert_eq!(
            r0.title.as_deref(),
            Some("PGFUZZ: Policy-guided fuzzing for robotic vehicles")
        );
        assert_eq!(r0.year, Some(2021)); // universal year merged into IEEE parse
        assert_eq!(
            out[1].as_ref().unwrap().venue.as_deref(),
            Some("Proc. of ICLR")
        );
        assert!(out[2].is_none()); // leftover -> LLM
    }

    #[test]
    fn parse_all_without_style_or_venue_returns_all_none() {
        // Three different signatures -> no 60% winner, no venue -> all None.
        let refs: Vec<String> = vec![
            r#"A. B, "Quoted title here," in Proc. X, 2020."#.into(),
            "Jane Doe. 2019. Some Title Here. In SOSP.".into(),
            "Roe, R.: Colon Title Here. In: S&P (2018)".into(),
        ];
        assert_eq!(parse_all(&refs, None), vec![None, None, None]);
        // Same input WITH an ACM venue: entry 2 parses, the others fail
        // ACM validation and stay None.
        let out = parse_all(&refs, Some("ACM CCS"));
        assert!(out[1].is_some());
        assert!(out[0].is_none() && out[2].is_none());
    }

    #[test]
    fn validation_rejects_wrong_but_plausible_parses() {
        // natbib author-year mis-voted into plain: "(2015)" glues onto an
        // author token, which fails the all-authors name-shape rule.
        let refs: Vec<String> = vec![
            "Kingma, D. P. and Ba, J. (2015). Adam: A method for stochastic optimization. In ICLR."
                .into(),
            "Sutton, R. S. and Barto, A. G. (2018). Reinforcement Learning: An Introduction. MIT Press."
                .into(),
        ];
        for r in parse_all(&refs, None) {
            assert!(r.is_none());
        }
        // URL-only entry: no title-shaped segment survives validation.
        let urls: Vec<String> =
            vec!["The mypy project. https://mypy-lang.org/. Accessed 2024.".into()];
        assert!(parse_all(&urls, None)[0].is_none());
    }
}
