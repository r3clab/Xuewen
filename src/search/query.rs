//! GitHub-style query syntax: `tag:nlp project:thesis is:starred
//! status:resolved in:title author:smith "phrase" free text`.
//! Parsing never errors — anything malformed degrades to free text.

use crate::search::fts::FieldSel;

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ParsedQuery {
    /// Leftover free text, original quoting preserved.
    pub(crate) text: String,
    /// `author:` terms (repeatable, ANDed, author-field-scoped).
    pub(crate) authors: Vec<String>,
    /// Union of `in:` tokens; None = all fields.
    pub(crate) fields: Option<FieldSel>,
    pub(crate) tag: Option<String>,
    /// Project NAME (resolution to id happens at the call boundary).
    pub(crate) project: Option<String>,
    pub(crate) starred: bool,
    /// Normalized: "resolved" | "needs_review".
    pub(crate) status: Option<String>,
}

/// One whitespace-separated token; quotes group spaces. `raw` is the exact
/// input slice; `key`/`value` are set when it looks like `key:value`.
struct Token<'a> {
    raw: &'a str,
    key: Option<String>,
    value: String,
}

fn tokenize(raw: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        let mut in_quotes = false;
        while i < bytes.len() && (in_quotes || !bytes[i].is_ascii_whitespace()) {
            if bytes[i] == b'"' {
                in_quotes = !in_quotes;
            }
            i += 1;
        }
        out.push(classify(&raw[start..i]));
    }
    out
}

/// Split `key:value` (key = ASCII letters, value non-empty after unquoting).
fn classify(tok: &str) -> Token<'_> {
    if let Some(colon) = tok.find(':') {
        let (key, rest) = (&tok[..colon], &tok[colon + 1..]);
        let value = rest.trim_matches('"');
        if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphabetic()) && !value.is_empty() {
            return Token {
                raw: tok,
                key: Some(key.to_ascii_lowercase()),
                value: value.to_string(),
            };
        }
    }
    Token {
        raw: tok,
        key: None,
        value: String::new(),
    }
}

pub(crate) fn parse(raw: &str) -> ParsedQuery {
    let mut q = ParsedQuery::default();
    let mut fields: Option<FieldSel> = None;
    let mut text: Vec<&str> = Vec::new();
    for t in tokenize(raw) {
        match t.key.as_deref() {
            Some("tag") => q.tag = Some(t.value),
            Some("project") => q.project = Some(t.value),
            Some("author") => q.authors.push(t.value),
            Some("is") if t.value.eq_ignore_ascii_case("starred") => q.starred = true,
            Some("status") => {
                let v = t.value.to_ascii_lowercase().replace('-', "_");
                match v.as_str() {
                    "resolved" | "needs_review" => q.status = Some(v),
                    _ => text.push(t.raw),
                }
            }
            Some("in") => {
                let sel = fields.get_or_insert(FieldSel::none());
                // `select` is shared with the `fields=` CSV so the qualifier
                // and the query parameter can never accept different spellings.
                if !sel.select(&t.value.to_ascii_lowercase()) {
                    text.push(t.raw);
                }
            }
            _ => text.push(t.raw),
        }
    }
    // An `in:` list that matched nothing valid means "no restriction".
    q.fields = fields.filter(|f| f.any());
    q.text = text.join(" ");
    q
}

/// Keyword-tier query: author terms as Tantivy field-scoped phrases, then
/// the free text. Embedded quotes are stripped (they'd break the phrase).
pub(crate) fn compose_keyword_query(authors: &[String], text: &str) -> String {
    let mut parts: Vec<String> = authors
        .iter()
        .map(|a| format!("authors:\"{}\"", a.replace('"', "")))
        .collect();
    if !text.is_empty() {
        parts.push(text.to_string());
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_words_are_free_text() {
        let p = parse("attention is all you need");
        assert_eq!(p.text, "attention is all you need");
        assert!(p.authors.is_empty() && p.tag.is_none() && p.project.is_none());
        assert!(!p.starred && p.status.is_none() && p.fields.is_none());
    }

    #[test]
    fn single_tag() {
        let p = parse("tag:nlp");
        assert_eq!(p.tag.as_deref(), Some("nlp"));
        assert_eq!(p.text, "");
    }

    #[test]
    fn quoted_values_and_all_filter_keys() {
        let p = parse(r#"tag:"deep learning" project:Thesis is:starred status:needs-review"#);
        assert_eq!(p.tag.as_deref(), Some("deep learning"));
        assert_eq!(p.project.as_deref(), Some("Thesis"));
        assert!(p.starred);
        assert_eq!(p.status.as_deref(), Some("needs_review"));
        assert_eq!(p.text, "");
    }

    #[test]
    fn in_tokens_union_fields() {
        let p = parse("in:title in:abstract transformers");
        let f = p.fields.unwrap();
        assert!(f.title && f.abstract_text && !f.authors && !f.body && !f.notes);
        assert_eq!(p.text, "transformers");
    }

    #[test]
    fn in_notes_scopes_to_annotation_notes() {
        let p = parse("in:notes attention");
        let f = p.fields.unwrap();
        assert!(f.notes && !f.title && !f.authors && !f.abstract_text && !f.body);
        assert_eq!(p.text, "attention");
    }

    #[test]
    fn author_terms_repeat() {
        let p = parse(r#"author:smith author:"ada lovelace" attention"#);
        assert_eq!(p.authors, vec!["smith", "ada lovelace"]);
        assert_eq!(p.text, "attention");
    }

    #[test]
    fn repeated_filter_key_keeps_last() {
        assert_eq!(parse("tag:a tag:b").tag.as_deref(), Some("b"));
    }

    #[test]
    fn unknown_keys_and_values_degrade_to_text() {
        let p = parse("foo:bar is:open in:everything tag:");
        assert_eq!(p.text, "foo:bar is:open in:everything tag:");
        assert!(p.tag.is_none() && !p.starred && p.fields.is_none());
    }

    #[test]
    fn quoted_phrases_pass_through() {
        assert_eq!(
            parse(r#""exact phrase" more"#).text,
            r#""exact phrase" more"#
        );
    }

    #[test]
    fn keys_case_insensitive_values_preserved() {
        let p = parse("TAG:NLP In:Title");
        assert_eq!(p.tag.as_deref(), Some("NLP"));
        assert!(p.fields.unwrap().title);
    }

    #[test]
    fn empty_input() {
        let p = parse("");
        assert_eq!(p.text, "");
        assert!(p.tag.is_none() && p.authors.is_empty() && p.fields.is_none());
    }

    #[test]
    fn unclosed_quote_runs_to_end() {
        assert_eq!(parse(r#"tag:"unclosed"#).tag.as_deref(), Some("unclosed"));
    }

    #[test]
    fn splits_only_on_ascii_whitespace_nbsp_stays_in_token() {
        // Pasted-from-PDF text carries NBSP; splitting is ASCII-only, so it
        // stays inside the value. The TS mirror pins the same case — JS `\s`
        // would split there and desync the pill bar from the server filter.
        let p = parse("tag:deep\u{00A0}learning");
        assert_eq!(p.tag.as_deref(), Some("deep\u{00A0}learning"));
        assert_eq!(p.text, "");
    }

    #[test]
    fn compose_prepends_author_scoped_terms() {
        assert_eq!(
            compose_keyword_query(&["smith".into()], "attention"),
            r#"authors:"smith" attention"#
        );
        assert_eq!(compose_keyword_query(&[], "x"), "x");
        assert_eq!(
            compose_keyword_query(&[r#"a "b""#.into()], ""),
            r#"authors:"a b""#
        );
    }
}
