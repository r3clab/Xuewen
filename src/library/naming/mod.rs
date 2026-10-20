pub(crate) mod files;

use std::collections::HashSet;

use unicode_normalization::UnicodeNormalization;

/// Leading title words to skip when choosing the cite-key title word.
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "on", "of", "in", "for", "to", "and", "or", "with", "at", "by", "from", "as",
    "is", "are", "be", "this", "that",
];

/// NFKD-fold to lowercase ASCII alphanumerics, joined (drops spaces, punctuation,
/// and diacritics). `"Müller-Groß"` → `"mullergro"`, `"Kaiming He!"` → `"kaiminghe"`.
pub(crate) fn fold_ascii_alnum(s: &str) -> String {
    s.nfkd()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Split into lowercase ASCII-alphanumeric runs (any non-alnum char is a boundary).
/// `"On Large-Batch Training"` → `["on", "large", "batch", "training"]`.
fn alnum_words(s: &str) -> Vec<String> {
    let decomposed: String = s.nfkd().collect();
    decomposed
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// Surname component: the folded last whitespace token of a full name.
/// `"Kaiming He"` → `Some("he")`, `"Laurens van der Maaten"` → `Some("maaten")`.
pub(crate) fn surname(full_name: &str) -> Option<String> {
    let last = full_name.split_whitespace().last()?;
    let folded = fold_ascii_alnum(last);
    (!folded.is_empty()).then_some(folded)
}

/// First title word after skipping leading stop words; if every word is a stop
/// word, falls back to the first word.
fn first_title_word(title: &str) -> Option<String> {
    let words = alnum_words(title);
    if let Some(w) = words.iter().find(|w| !STOP_WORDS.contains(&w.as_str())) {
        return Some(w.clone());
    }
    words.into_iter().next()
}

/// The base cite key `{surname}{year}{titleword}`, or `None` if the first author,
/// the year, or a usable title word is missing.
pub(crate) fn cite_key_base(
    authors: &[String],
    year: Option<i64>,
    title: Option<&str>,
) -> Option<String> {
    let surname = surname(authors.first()?)?;
    let year = year?;
    let word = first_title_word(title?)?;
    Some(format!("{surname}{year}{word}"))
}

/// A free cite key: `base` if untaken, else `base` + `a`..`z`, then numeric.
pub(crate) fn disambiguate(base: &str, taken: &HashSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_string();
    }
    for c in b'a'..=b'z' {
        let cand = format!("{base}{}", c as char);
        if !taken.contains(&cand) {
            return cand;
        }
    }
    let mut n = 2;
    loop {
        let cand = format!("{base}{n}");
        if !taken.contains(&cand) {
            return cand;
        }
        n += 1;
    }
}

/// Relative library path: `<citekey>.pdf`, or `_unsorted/<hash>.pdf` when no key.
pub(crate) fn library_rel_path(cite_key: Option<&str>, content_hash: &str) -> String {
    match cite_key {
        Some(key) => format!("{key}.pdf"),
        None => format!("_unsorted/{content_hash}.pdf"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_diacritics_and_punctuation() {
        assert_eq!(fold_ascii_alnum("Müller"), "muller");
        assert_eq!(fold_ascii_alnum("Kaiming He!"), "kaiminghe");
    }

    #[test]
    fn surname_is_last_token() {
        assert_eq!(surname("Kaiming He").as_deref(), Some("he"));
        assert_eq!(surname("Laurens van der Maaten").as_deref(), Some("maaten"));
        assert_eq!(surname("   ").as_deref(), None);
    }

    #[test]
    fn title_word_skips_stop_words() {
        assert_eq!(
            first_title_word("A Neural Probabilistic Language Model").as_deref(),
            Some("neural")
        );
        assert_eq!(
            first_title_word("Attention Is All You Need").as_deref(),
            Some("attention")
        );
        assert_eq!(
            first_title_word("On Large-Batch Training Methods").as_deref(),
            Some("large")
        );
        assert_eq!(
            first_title_word("Deep Residual Learning").as_deref(),
            Some("deep")
        );
    }

    #[test]
    fn builds_and_requires_all_parts() {
        let authors = vec!["Kaiming He".to_string()];
        assert_eq!(
            cite_key_base(
                &authors,
                Some(2016),
                Some("Deep Residual Learning for Image Recognition")
            )
            .as_deref(),
            Some("he2016deep")
        );
        assert_eq!(cite_key_base(&[], Some(2016), Some("x")), None);
        assert_eq!(cite_key_base(&authors, None, Some("x")), None);
        assert_eq!(cite_key_base(&authors, Some(2016), None), None);
    }

    #[test]
    fn disambiguation_appends_letters() {
        let mut taken = HashSet::new();
        assert_eq!(disambiguate("he2016deep", &taken), "he2016deep");
        taken.insert("he2016deep".to_string());
        assert_eq!(disambiguate("he2016deep", &taken), "he2016deepa");
        taken.insert("he2016deepa".to_string());
        assert_eq!(disambiguate("he2016deep", &taken), "he2016deepb");
    }

    #[test]
    fn rel_path_keyed_vs_unsorted() {
        assert_eq!(
            library_rel_path(Some("he2016deep"), "abc"),
            "he2016deep.pdf"
        );
        assert_eq!(library_rel_path(None, "abc123"), "_unsorted/abc123.pdf");
    }
}
