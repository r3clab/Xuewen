use crate::library::model::Identifier;
use regex::Regex;
use std::sync::LazyLock;

static DOI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"10\.\d{4,9}/[-._;()/:A-Za-z0-9]+").unwrap());
static ARXIV_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)arxiv:\s*(\d{4}\.\d{4,5}(?:v\d+)?)").unwrap());

pub(crate) fn extract_doi(text: &str) -> Option<String> {
    DOI_RE.find(text).map(|m| {
        m.as_str()
            .trim_end_matches(['.', ',', ')', ';'])
            .to_string()
    })
}

fn extract_arxiv(text: &str) -> Option<String> {
    ARXIV_RE.captures(text).map(|c| c[1].to_string())
}

/// Prefer a DOI (published record) over an arXiv id (preprint) when both
/// appear. The extracted strings pass through the canonicalizing constructors
/// so a printed-case DOI or a versioned arXiv banner dedupes against the
/// resolver's form of the same identifier.
pub(crate) fn identify(text: &str) -> Identifier {
    if let Some(doi) = extract_doi(text) {
        return Identifier::doi(doi);
    }
    if let Some(id) = extract_arxiv(text) {
        return Identifier::arxiv(id);
    }
    Identifier::None
}

/// Words that indicate a line ended mid-phrase (a wrapped title).
const JOIN_WORDS: &[&str] = &[
    "a", "an", "and", "by", "for", "from", "in", "of", "on", "or", "the", "to", "via", "with",
];

/// Venue boilerplate a conference template stamps above the title — ICLR's
/// "Published as a conference paper at ICLR 2026" running header is the
/// motivating case. Such a line passes every other test here (long, alphabetic,
/// no email, no DOI), so without this it becomes the guessed title and the
/// search that follows can only miss.
static BOILERPLATE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(?:a\s+)?preprint\b|\b(?:under review|published as a|accepted (?:as|at|to)|submitted to|to appear in|proceedings of the|camera[ -]ready)\b",
    )
    .unwrap()
});

/// A line usable as (part of) a title under the existing filter rules.
fn substantive(line: &str) -> Option<&str> {
    let t = line.trim();
    (t.len() >= 8
        && !t.to_lowercase().starts_with("arxiv")
        && !t.contains('@')
        && !DOI_RE.is_match(t)
        && !BOILERPLATE_RE.is_match(t)
        && t.chars().any(|c| c.is_alphabetic()))
    .then_some(t)
}

static SMALLCAPS_SPLIT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b([A-Z]) ([A-Z]{2,})\b").unwrap());

/// pdftotext's small-caps artifact on an all-caps line, repaired without a
/// reference title (`pdf::repair_smallcaps` repairs the same artifact in body
/// text, but only once a resolved title says which words to rejoin).
///
/// The ICLR and NeurIPS templates set titles with `\sc`, which renders each
/// word's first letter a size larger than the rest; pdftotext breaks at that
/// font switch and emits "T RANSFORMERS ARE I NHERENTLY S UCCINCT".
///
/// Two guards, because "A NOVEL APPROACH" is a legitimately uppercase line
/// whose "A" is a real word: the line must carry no lowercase at all, and at
/// least one split-off letter must be something other than "A" or "I" — the
/// only one-letter English words, and a distinction no font artifact would
/// respect. Returns `None` when the line shows no artifact.
fn repair_smallcaps_line(line: &str) -> Option<String> {
    if line.chars().any(char::is_lowercase) {
        return None;
    }
    SMALLCAPS_SPLIT_RE
        .captures_iter(line)
        .any(|c| !matches!(&c[1], "A" | "I"))
        .then(|| {
            SMALLCAPS_SPLIT_RE
                .replace_all(line, "${1}${2}")
                .into_owned()
        })
}

/// A substantive line with pdftotext's small-caps splits rejoined.
fn title_line(line: &str) -> Option<String> {
    let t = substantive(line)?;
    Some(repair_smallcaps_line(t).unwrap_or_else(|| t.to_string()))
}

/// Whether a title line ends "mid-phrase" (wrapped onto the next line).
fn ends_mid_phrase(line: &str) -> bool {
    if line.ends_with(':') || line.ends_with('-') {
        return true;
    }
    line.rsplit(|c: char| !c.is_alphanumeric())
        .find(|w| !w.is_empty())
        .is_some_and(|w| JOIN_WORDS.contains(&w.to_lowercase().as_str()))
}

/// Best-effort provisional title: the first substantive line of the header
/// text, joined with the next line when the first ends mid-phrase (wrapped
/// two-line titles are common on conference cover sheets).
pub(crate) fn guess_title(text: &str) -> Option<String> {
    // `loop`+`next()` rather than `while let` (clippy::while_let_on_iterator):
    // the join branch needs to pull a second line from the same iterator.
    let mut lines = text.lines();
    loop {
        let line = lines.next()?;
        let Some(first) = title_line(line) else {
            continue;
        };
        if ends_mid_phrase(&first) {
            if let Some(next) = lines.next().and_then(title_line) {
                if let Some(stem) = first.strip_suffix('-') {
                    // A trailing '-' is a mid-word split ("Hyperdimen-" / "sional") only
                    // when both sides are lowercase; otherwise it's a hyphenated compound
                    // wrapped at the hyphen ("State-of-the-" / "Art") — keep the hyphen.
                    let mid_word = stem.chars().last().is_some_and(|c| c.is_lowercase())
                        && next.chars().next().is_some_and(|c| c.is_lowercase());
                    return Some(if mid_word {
                        format!("{stem}{next}")
                    } else {
                        format!("{first}{next}")
                    });
                }
                return Some(format!("{first} {next}"));
            }
        }
        return Some(first);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_doi() {
        let text = "See https://doi.org/10.1145/3292500.3330701 for details.";
        assert_eq!(
            extract_doi(text).as_deref(),
            Some("10.1145/3292500.3330701")
        );
    }

    #[test]
    fn finds_arxiv() {
        assert_eq!(
            extract_arxiv("arXiv:1706.03762v5").as_deref(),
            Some("1706.03762v5")
        );
        assert_eq!(
            extract_arxiv("arXiv: 2001.00001").as_deref(),
            Some("2001.00001")
        );
    }

    #[test]
    fn doi_wins_over_arxiv() {
        let text = "arXiv:1706.03762  doi:10.1145/3292500.3330701";
        assert_eq!(
            identify(text),
            Identifier::Doi("10.1145/3292500.3330701".into())
        );
    }

    #[test]
    fn no_identifier() {
        assert_eq!(identify("Just some prose with no ids."), Identifier::None);
    }

    #[test]
    fn identify_canonicalizes_what_it_extracts() {
        // The banner's version suffix and the publisher's printed case are
        // extraction detail; the identifier boundary strips/folds them.
        assert_eq!(
            identify("arXiv:1706.03762v5 [cs.CL]"),
            Identifier::Arxiv("1706.03762".into())
        );
        assert_eq!(
            identify("doi:10.1109/TSE.2019.2946563"),
            Identifier::Doi("10.1109/tse.2019.2946563".into())
        );
    }

    #[test]
    fn guesses_title_skipping_arxiv_banner() {
        let text =
            "arXiv:1706.03762v5 [cs.CL] 6 Dec 2017\nAttention Is All You Need\nAshish Vaswani";
        assert_eq!(
            guess_title(text).as_deref(),
            Some("Attention Is All You Need")
        );
    }

    #[test]
    fn joins_wrapped_title_ending_mid_phrase() {
        // Exact shape of the motivating USENIX cover sheet.
        let text = "AntiFuzz: Impeding Fuzzing Audits of\nBinary Executables\nEmre Güler, Cornelius Aschermann, Ali Abbasi, and Thorsten Holz,\nRuhr-Universität Bochum";
        assert_eq!(
            guess_title(text).as_deref(),
            Some("AntiFuzz: Impeding Fuzzing Audits of Binary Executables")
        );
    }

    #[test]
    fn joins_after_trailing_colon_and_dehyphenates() {
        assert_eq!(
            guess_title("Some System:\nA Grand Unified Theory\nAuthor Name").as_deref(),
            Some("Some System: A Grand Unified Theory")
        );
        assert_eq!(
            guess_title("Hyperdimen-\nsional Computing Systems\nAuthor Name").as_deref(),
            Some("Hyperdimensional Computing Systems")
        );
        assert_eq!(
            guess_title("Towards State-of-the-\nArt Adversarial Robustness\nAuthor Name")
                .as_deref(),
            Some("Towards State-of-the-Art Adversarial Robustness")
        );
    }

    #[test]
    fn does_not_join_complete_titles() {
        // First line doesn't end mid-phrase: next line (authors) is not joined.
        let text = "Attention Is All You Need\nAshish Vaswani, Noam Shazeer";
        assert_eq!(
            guess_title(text).as_deref(),
            Some("Attention Is All You Need")
        );
    }

    #[test]
    fn rejoins_iclr_small_caps_title() {
        // Exact pdftotext output for the ICLR camera-ready of arXiv:2510.19315,
        // whose `\sc` title splits every word at the font switch.
        let text = "Published as a conference paper at ICLR 2026\n\nT RANSFORMERS ARE I NHERENTLY S UCCINCT\n\nPascal Bergsträßer\nRyan Cotterell";
        assert_eq!(
            guess_title(text).as_deref(),
            Some("TRANSFORMERS ARE INHERENTLY SUCCINCT")
        );
    }

    #[test]
    fn skips_conference_running_headers() {
        for header in [
            "Published as a conference paper at ICLR 2026",
            "Under review as a conference paper at ICLR 2026",
            "Accepted at NeurIPS 2024",
            "Preprint. Under review.",
            "Proceedings of the 41st International Conference on Machine Learning",
        ] {
            let text = format!("{header}\nAttention Is All You Need\nAshish Vaswani");
            assert_eq!(
                guess_title(&text).as_deref(),
                Some("Attention Is All You Need"),
                "expected {header:?} to be skipped"
            );
        }
    }

    #[test]
    fn leaves_a_genuinely_uppercase_title_alone() {
        // "A" is a real one-letter word, so an all-caps line whose only
        // single-letter fragments are "A"/"I" is not a small-caps artifact.
        assert_eq!(
            guess_title("A NOVEL APPROACH TO A BETTER CACHE\nAuthor Name").as_deref(),
            Some("A NOVEL APPROACH TO A BETTER CACHE")
        );
        // A title case line is never touched, whatever spacing it carries.
        assert_eq!(
            guess_title("A Novel Approach to Caching\nAuthor Name").as_deref(),
            Some("A Novel Approach to Caching")
        );
    }

    #[test]
    fn small_caps_repair_keeps_real_one_letter_words() {
        // "A S URVEY" — the standalone "A" survives because it is followed by
        // a single letter, and "S URVEY" rejoins because "S" is not a word.
        assert_eq!(
            guess_title("A S URVEY OF I MAGE M ODELS\nAuthor Name").as_deref(),
            Some("A SURVEY OF IMAGE MODELS")
        );
    }

    #[test]
    fn does_not_join_when_next_line_is_not_substantive() {
        // Next line carries an email -> not substantive -> no join.
        let text = "A Study of\nauthor@example.com things\nReal Second Line";
        assert_eq!(guess_title(text).as_deref(), Some("A Study of"));
    }
}
