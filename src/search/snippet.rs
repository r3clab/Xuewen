//! Match excerpts as runs of plain text with the query's hits marked. Markup
//! is the renderer's business: `<mark>` in the web UI, nothing in a terminal.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(transparent)]
#[ts(export)]
pub struct Snippet(pub Vec<SnippetPart>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SnippetPart {
    pub text: String,
    pub marked: bool,
}

impl Snippet {
    pub(crate) fn plain(text: &str) -> Self {
        let mut s = Self::default();
        s.push(text, false);
        s
    }

    /// Tantivy's fragment with its highlighted byte ranges marked; `None`
    /// when nothing in it matched.
    pub(crate) fn highlighted(snippet: &tantivy::snippet::Snippet) -> Option<Self> {
        if snippet.highlighted().is_empty() {
            return None;
        }
        let fragment = snippet.fragment();
        let mut ranges = snippet.highlighted().to_vec();
        ranges.sort_by_key(|r| r.start);
        let mut s = Self::default();
        let mut at = 0;
        for r in ranges {
            let start = r.start.max(at);
            if start >= r.end {
                continue;
            }
            s.push(&fragment[at..start], false);
            s.push(&fragment[start..r.end], true);
            at = r.end;
        }
        s.push(&fragment[at..], false);
        Some(s)
    }

    pub fn text(&self) -> String {
        self.0.iter().map(|p| p.text.as_str()).collect()
    }

    fn push(&mut self, text: &str, marked: bool) {
        if text.is_empty() {
            return;
        }
        match self.0.last_mut() {
            Some(last) if last.marked == marked => last.text.push_str(text),
            _ => self.0.push(SnippetPart {
                text: text.to_string(),
                marked,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(text: &str, marked: bool) -> SnippetPart {
        SnippetPart {
            text: text.into(),
            marked,
        }
    }

    #[test]
    fn plain_text_is_one_unmarked_run_and_empty_text_none() {
        assert_eq!(
            Snippet::plain("Turing's <test>").0,
            vec![part("Turing's <test>", false)]
        );
        assert!(Snippet::plain("").0.is_empty());
    }

    #[test]
    fn serializes_as_a_bare_array_of_runs() {
        let s = Snippet(vec![part("a ", false), part("b", true)]);
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"[{"text":"a ","marked":false},{"text":"b","marked":true}]"#
        );
    }
}
