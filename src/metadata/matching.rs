use std::collections::HashSet;

use strsim::normalized_levenshtein;

use crate::library::naming;

/// Similarity (0.0–1.0) at or above which a candidate title is accepted as a match.
pub(crate) const MATCH_THRESHOLD: f64 = 0.85;

/// Lowercase, replace every non-alphanumeric char with a space, and collapse whitespace.
fn normalize_title(s: &str) -> String {
    let spaced: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .to_lowercase();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Normalized-Levenshtein similarity of two titles after normalization.
pub(crate) fn title_similarity(a: &str, b: &str) -> f64 {
    normalized_levenshtein(&normalize_title(a), &normalize_title(b))
}

/// Folded surnames shorter than this do not count as evidence: "He", "Li",
/// "Xu" and "Yu" are ubiquitous CS surnames *and* ordinary words of an English
/// abstract, so finding one in a paper's text says nothing about whether the
/// record claiming it describes that paper.
const MIN_EVIDENCE_LEN: usize = 3;

/// The paper's own extracted text, folded word by word so that a candidate
/// record's claimed authors can be checked against it.
///
/// Title similarity alone cannot establish that a candidate *is* the work in
/// hand. Crossref carries a book chapter whose title is character-for-character
/// the GPT-3 paper's and which therefore scores a perfect 1.00; its sole author
/// appears nowhere in the GPT-3 PDF. The record's publication type cannot make
/// that call — Springer registers LNCS proceedings (ECCV, MICCAI) as
/// `book-chapter` too, so the genuine Microsoft COCO record is the same type at
/// the same perfect score — but its eight authors do all appear in the COCO
/// PDF. Authorship separates the two; the type does not.
#[derive(Debug, Default)]
pub(crate) struct PaperText {
    words: HashSet<String>,
}

impl PaperText {
    /// Fold `text` into its set of comparable words.
    ///
    /// Tokenized on whitespace and folded with the *same* function
    /// `naming::surname` applies to a candidate's author, so the two sides
    /// cannot drift: both drop diacritics, punctuation and hyphens rather than
    /// splitting on them, which is what lets "Dollár,", "Müller-Groß" and
    /// "Bergsträßer" in a PDF compare equal to the surnames of those names.
    ///
    /// A superscript affiliation marker arrives glued to the name it marks,
    /// with no space for the tokenizer to break on — the Segment Anything
    /// header extracts as "Alexander Kirillov1,2,4 Eric Mintun2" — and NFKD
    /// turns those superscripts into ordinary digits rather than dropping them.
    /// Each such token is therefore indexed under its digit-stripped stem as
    /// well, or a paper whose every author is affiliated would corroborate
    /// nothing.
    pub(crate) fn new(text: &str) -> Self {
        let mut words = HashSet::new();
        for token in text.split_whitespace() {
            let folded = naming::fold_ascii_alnum(token);
            if folded.is_empty() {
                continue;
            }
            let stem = folded.trim_end_matches(|c: char| c.is_ascii_digit());
            if !stem.is_empty() && stem.len() < folded.len() {
                words.insert(stem.to_string());
            }
            words.insert(folded);
        }
        Self { words }
    }

    /// Whether the paper's text corroborates `authors`: at least one surname
    /// long enough to be evidence appears in it.
    ///
    /// Abstains — returns `true` — when there is no evidence either way, i.e.
    /// the candidate names no authors, or the text yielded no words to look
    /// them up in. A scanned PDF that extracts to nothing must keep resolving
    /// exactly as well as it did before this check existed; only a record the
    /// paper in hand positively contradicts is rejected.
    pub(crate) fn corroborates(&self, authors: &[String]) -> bool {
        if self.words.is_empty() {
            return true;
        }
        let mut evidence = authors
            .iter()
            .filter_map(|a| naming::surname(a))
            .filter(|s| s.len() >= MIN_EVIDENCE_LEN)
            .peekable();
        evidence.peek().is_none() || evidence.any(|s| self.words.contains(&s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_punctuation_and_case() {
        assert_eq!(
            normalize_title("KGAT: Knowledge-Graph  Attention Network!"),
            "kgat knowledge graph attention network"
        );
    }

    #[test]
    fn identical_titles_clear_the_threshold() {
        let q = "KGAT: Knowledge Graph Attention Network for Recommendation";
        let c = "KGAT: Knowledge Graph Attention Network for Recommendation.";
        assert!(title_similarity(q, c) >= MATCH_THRESHOLD);
    }

    #[test]
    fn unrelated_titles_fall_below_the_threshold() {
        assert!(
            title_similarity(
                "Deep Residual Learning for Image Recognition",
                "Attention Is All You Need"
            ) < MATCH_THRESHOLD
        );
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn separates_the_two_perfect_title_matches() {
        // The motivating pair, both `book-chapter` in Crossref and both scoring
        // 1.00 against their query. Only authorship tells them apart.
        let gpt3 = PaperText::new(
            "Language Models are Few-Shot Learners\n\nTom B. Brown Benjamin Mann Nick Ryder\nOpenAI",
        );
        assert!(!gpt3.corroborates(&names(&["Sourav Malakar"])));
        assert!(gpt3.corroborates(&names(&["Tom B. Brown", "Benjamin Mann"])));
    }

    #[test]
    fn one_matching_surname_is_enough() {
        let t = PaperText::new("Microsoft COCO: Common Objects in Context\nTsung-Yi Lin, Piotr Dollár, C. Lawrence Zitnick");
        // Diacritics and hyphens fold away on both sides.
        assert!(t.corroborates(&names(&["Piotr Dollár"])));
        assert!(t.corroborates(&names(&["Nobody At All", "Serge Zitnick"])));
        assert!(!t.corroborates(&names(&["Nobody At All", "Some Other Person"])));
    }

    #[test]
    fn abstains_without_evidence_either_way() {
        let t = PaperText::new("A Paper By Someone\nAda Lovelace");
        assert!(t.corroborates(&[]));
        // Nothing extracted from the PDF: resolve exactly as before this check.
        assert!(PaperText::default().corroborates(&names(&["Sourav Malakar"])));
        assert!(PaperText::new("   ").corroborates(&names(&["Sourav Malakar"])));
    }

    #[test]
    fn affiliation_markers_do_not_hide_an_author() {
        // The Segment Anything header, as pdftotext renders it: every surname
        // arrives with its affiliation superscripts glued on.
        let t = PaperText::new("Alexander Kirillov1,2,4 Eric Mintun2 Nikhila Ravi1,2");
        assert!(t.corroborates(&names(&["Eric Mintun"])));
        assert!(t.corroborates(&names(&["Alexander Kirillov", "Nikhila Ravi"])));
        assert!(!t.corroborates(&names(&["Sourav Malakar"])));
    }

    #[test]
    fn short_surnames_are_not_evidence() {
        // "he" is a pronoun in any abstract, so it can neither confirm nor
        // condemn a record: a candidate with only short surnames abstains.
        let t = PaperText::new("We show that he improves on the prior state of the art.");
        assert!(t.corroborates(&names(&["Kaiming He"])));
        // Once a usable surname exists the check bites, and the short one
        // cannot rescue a record the paper contradicts.
        assert!(!t.corroborates(&names(&["Kaiming He", "Sourav Malakar"])));
    }
}
