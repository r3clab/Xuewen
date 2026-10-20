use crate::library::model::Identifier;
use regex::Regex;
use std::sync::LazyLock;

/// A classified import input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Source {
    Arxiv(String),
    Doi(String),
    IeeeDocument(String),
}

/// A resolved PDF location and whether it must be fetched through the proxy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PdfTarget {
    pub(crate) url: String,
    pub(crate) requires_proxy: bool,
}

/// arXiv id preceded by an explicit "arXiv:" marker or an arxiv.org abs/pdf URL.
/// The prefix is REQUIRED so a bare number sitting in prose (even prose that
/// mentions "arxiv") is not misread as an id.
static ARXIV_CONTEXT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:arxiv:\s*|arxiv\.org/(?:abs|pdf)/)(\d{4}\.\d{4,5}(?:v\d+)?)").unwrap()
});
/// A bare arXiv id occupying the whole (trimmed) input.
static ARXIV_BARE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{4}\.\d{4,5}(?:v\d+)?$").unwrap());
static IEEE_DOC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)ieeexplore\.ieee\.org/document/(\d+)").unwrap());

/// Classify a pasted input into a `Source`. Order matters: IEEE document URL,
/// then any DOI, then arXiv (explicit context, else a bare whole-input id).
pub(crate) fn parse_source(input: &str) -> Option<Source> {
    let t = input.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(c) = IEEE_DOC_RE.captures(t) {
        return Some(Source::IeeeDocument(c[1].to_string()));
    }
    // A DOI anywhere (including doi.org / dl.acm.org URLs). Reuse identify's
    // extractor so the DOI pattern stays defined in one place.
    if let Some(doi) = crate::metadata::identify::extract_doi(t) {
        return Some(Source::Doi(doi));
    }
    // arXiv: an explicit "arXiv:"/arxiv.org context, else a bare id that is the
    // entire input (tolerating trailing punctuation from a paste).
    if let Some(c) = ARXIV_CONTEXT_RE.captures(t) {
        return Some(Source::Arxiv(c[1].to_string()));
    }
    let bare = t.trim_end_matches(['.', ',', ';']);
    if ARXIV_BARE_RE.is_match(bare) {
        return Some(Source::Arxiv(bare.to_string()));
    }
    None
}

/// Map a source to its PDF URL, or `None` when no publisher PDF URL is
/// constructible (unknown publisher, or an IEEE DOI without an arnumber).
pub(crate) fn pdf_target(src: &Source) -> Option<PdfTarget> {
    match src {
        Source::Arxiv(id) => Some(PdfTarget {
            url: format!("https://arxiv.org/pdf/{id}"),
            requires_proxy: false,
        }),
        Source::Doi(doi) if doi.starts_with("10.1145/") => Some(PdfTarget {
            url: format!("https://dl.acm.org/doi/pdf/{doi}"),
            requires_proxy: true,
        }),
        Source::IeeeDocument(arnumber) => Some(PdfTarget {
            url: format!("https://ieeexplore.ieee.org/stamp/stamp.jsp?arnumber={arnumber}"),
            requires_proxy: true,
        }),
        Source::Doi(_) => None,
    }
}

/// The identifier a source implies, used to seed metadata resolution during
/// ingest. An IEEE arnumber is not a DOI, so it yields no hint. Canonicalized
/// here rather than in `parse_source`: the `Source` keeps the pasted form
/// verbatim because `pdf_target` builds fetch URLs from it (an explicit
/// `…v5` should fetch that version), while the identifier must dedupe.
pub(crate) fn source_identifier(src: &Source) -> Option<Identifier> {
    match src {
        Source::Doi(d) => Some(Identifier::doi(d.clone())),
        Source::Arxiv(a) => Some(Identifier::arxiv(a.clone())),
        Source::IeeeDocument(_) => None,
    }
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    #[test]
    fn parses_arxiv_forms() {
        for s in [
            "1706.03762",
            "arXiv:1706.03762",
            "arxiv:1706.03762v5",
            "https://arxiv.org/abs/1706.03762",
            "https://arxiv.org/pdf/1706.03762v5",
        ] {
            assert!(matches!(parse_source(s), Some(Source::Arxiv(_))), "{s}");
        }
        assert_eq!(
            parse_source("1706.03762v5"),
            Some(Source::Arxiv("1706.03762v5".into()))
        );
    }

    #[test]
    fn parses_doi_forms() {
        assert_eq!(
            parse_source("10.1145/3292500.3330701"),
            Some(Source::Doi("10.1145/3292500.3330701".into()))
        );
        assert_eq!(
            parse_source("https://doi.org/10.1145/3292500.3330701"),
            Some(Source::Doi("10.1145/3292500.3330701".into()))
        );
        // ACM landing URL carries the DOI in its path.
        assert_eq!(
            parse_source("https://dl.acm.org/doi/10.1145/3292500.3330701"),
            Some(Source::Doi("10.1145/3292500.3330701".into()))
        );
    }

    #[test]
    fn parses_ieee_document_url() {
        assert_eq!(
            parse_source("https://ieeexplore.ieee.org/document/8835311"),
            Some(Source::IeeeDocument("8835311".into()))
        );
    }

    #[test]
    fn rejects_junk() {
        assert_eq!(parse_source(""), None);
        assert_eq!(parse_source("just a title of a paper"), None);
        assert_eq!(parse_source("https://example.com/thing"), None);
    }

    #[test]
    fn pdf_target_arxiv_is_open() {
        let t = pdf_target(&Source::Arxiv("1706.03762".into())).unwrap();
        assert_eq!(t.url, "https://arxiv.org/pdf/1706.03762");
        assert!(!t.requires_proxy);
    }

    #[test]
    fn pdf_target_acm_needs_proxy() {
        let t = pdf_target(&Source::Doi("10.1145/3292500.3330701".into())).unwrap();
        assert_eq!(t.url, "https://dl.acm.org/doi/pdf/10.1145/3292500.3330701");
        assert!(t.requires_proxy);
    }

    #[test]
    fn pdf_target_ieee_document_needs_proxy() {
        let t = pdf_target(&Source::IeeeDocument("8835311".into())).unwrap();
        assert_eq!(
            t.url,
            "https://ieeexplore.ieee.org/stamp/stamp.jsp?arnumber=8835311"
        );
        assert!(t.requires_proxy);
    }

    #[test]
    fn pdf_target_unknown_doi_is_none() {
        // A non-ACM/IEEE DOI has no constructible publisher PDF URL.
        assert!(pdf_target(&Source::Doi("10.1109/5.771073".into())).is_none()); // IEEE DOI: no arnumber
        assert!(pdf_target(&Source::Doi("10.1000/xyz".into())).is_none());
    }

    #[test]
    fn source_identifier_maps_doi_and_arxiv() {
        assert_eq!(
            source_identifier(&Source::Doi("10.1/x".into())),
            Some(Identifier::Doi("10.1/x".into()))
        );
        assert_eq!(
            source_identifier(&Source::Arxiv("1706.03762".into())),
            Some(Identifier::Arxiv("1706.03762".into()))
        );
        assert_eq!(
            source_identifier(&Source::IeeeDocument("8835311".into())),
            None
        );
    }

    #[test]
    fn source_identifier_canonicalizes_while_source_stays_verbatim() {
        // The Source keeps the pasted form (pdf_target fetches exactly v5)…
        let src = parse_source("arXiv:1706.03762v5").unwrap();
        assert_eq!(src, Source::Arxiv("1706.03762v5".into()));
        // …while the dedupe/resolution identifier is version-stripped.
        assert_eq!(
            source_identifier(&src),
            Some(Identifier::Arxiv("1706.03762".into()))
        );
    }

    #[test]
    fn ignores_bare_id_embedded_in_prose() {
        // "arxiv" in surrounding prose must not turn a stray number into an id.
        assert_eq!(
            parse_source("arxiv preprint, see paper 1234.5678 in the appendix"),
            None
        );
    }

    #[test]
    fn bare_arxiv_id_tolerates_trailing_punctuation() {
        assert_eq!(
            parse_source("1706.03762."),
            Some(Source::Arxiv("1706.03762".into()))
        );
    }

    #[test]
    fn doi_takes_priority_over_arxiv_context() {
        // Both an arXiv marker and a DOI present → DOI wins (checked first).
        assert_eq!(
            parse_source("arXiv:1706.03762 also 10.1145/3292500.3330701"),
            Some(Source::Doi("10.1145/3292500.3330701".into()))
        );
    }
}
