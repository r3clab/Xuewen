//! GitHub-style query syntax: `tag:nlp project:thesis is:starred
//! status:resolved in:title author:smith "phrase" free text`.
//! Parsing never errors — anything malformed degrades to free text.

use anyhow::Result;
use sqlx::SqlitePool;

use crate::library::model::PaperStatus;
use crate::library::papers::PaperFilter;
use crate::library::projects;
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
    pub(crate) status: Option<PaperStatus>,
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
            Some("status") => match t.value.to_ascii_lowercase().replace('-', "_").as_str() {
                "resolved" => q.status = Some(PaperStatus::Resolved),
                "needs_review" => q.status = Some(PaperStatus::NeedsReview),
                _ => text.push(t.raw),
            },
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

impl ParsedQuery {
    /// The list restrictions the qualifiers spell. A `project:` name that
    /// names no project binds the name itself, which can never equal a
    /// project id, so the list filters to nothing rather than ignoring it.
    pub(crate) async fn filter(&self, pool: &SqlitePool) -> Result<PaperFilter> {
        let project = match &self.project {
            Some(name) => Some(
                projects::find_by_name(pool, name)
                    .await?
                    .map_or_else(|| name.clone(), |p| p.id),
            ),
            None => None,
        };
        Ok(PaperFilter {
            status: self.status,
            project,
            tag: self.tag.clone(),
            starred: self.starred.then_some(true),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// The parse as the shared fixture spells it, field for field.
    fn as_fixture(p: &ParsedQuery) -> Value {
        let fields = p.fields.map(|f| {
            [
                ("title", f.title),
                ("authors", f.authors),
                ("abstract", f.abstract_text),
                ("body", f.body),
                ("notes", f.notes),
            ]
            .into_iter()
            .filter_map(|(name, on)| on.then_some(name))
            .collect::<Vec<_>>()
        });
        json!({
            "text": p.text,
            "authors": p.authors,
            "fields": fields,
            "tag": p.tag,
            "project": p.project,
            "starred": p.starred,
            "status": p.status,
        })
    }

    /// The fixture the frontend's mirror of this parser runs against too
    /// (frontend/src/features/library/search/query.test.ts).
    fn fixture() -> Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/frontend/src/features/library/search/query_cases.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn parses_every_shared_case() {
        let fixture = fixture();
        for case in fixture["cases"].as_array().unwrap() {
            let input = case["input"].as_str().unwrap();
            assert_eq!(as_fixture(&parse(input)), case["parsed"], "{input:?}");
        }
    }

    #[test]
    fn semantic_fields_match_the_shared_spec() {
        let fixture = fixture();
        let spec: Vec<&str> = fixture["semantic_fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for name in ["title", "authors", "abstract", "body", "notes"] {
            let mut only = FieldSel::none();
            assert!(only.select(name));
            assert_eq!(only.semantic_applicable(), spec.contains(&name), "{name}");
        }
    }

    #[tokio::test]
    async fn filter_resolves_a_project_name_and_keeps_an_unknown_one() {
        let (_dir, pool) = crate::testutil::temp_pool().await;
        let thesis = crate::library::projects::create(
            &pool,
            &crate::library::projects::ProjectName::parse("Thesis").unwrap(),
        )
        .await
        .unwrap();
        let f = parse("project:thesis tag:nlp is:starred status:resolved")
            .filter(&pool)
            .await
            .unwrap();
        assert_eq!(f.project.as_deref(), Some(thesis.id.as_str()));
        assert_eq!(f.tag.as_deref(), Some("nlp"));
        assert_eq!(f.starred, Some(true));
        assert_eq!(f.status, Some(PaperStatus::Resolved));

        let unknown = parse("project:nosuch").filter(&pool).await.unwrap();
        assert_eq!(unknown.project.as_deref(), Some("nosuch"));
        assert_eq!(unknown.starred, None);
    }
}
