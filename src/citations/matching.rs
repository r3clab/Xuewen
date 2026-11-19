//! Which works a paper's bibliography cites are already in the library.

use serde::Deserialize;

use crate::library::model::Paper;
use crate::metadata::matching::normalize_title;

/// Normalized titles shorter than this turn up by chance in prose ("On It"),
/// so they never match.
const MIN_TITLE_LEN: usize = 12;

/// One bibliography entry as the reader extracted it, plus its parsed title
/// once the structured parse has run.
#[derive(Debug, Deserialize, ts_rs::TS)]
#[ts(export)]
pub(crate) struct CitedReference {
    pub(crate) text: String,
    #[serde(default)]
    #[ts(optional)]
    pub(crate) title: Option<String>,
}

/// For each reference, the first of `papers` it cites: the one whose
/// normalized title equals the reference's parsed title, else the first whose
/// title occurs as whole words inside the reference text (an entry almost
/// always spells the cited title verbatim, so no parsing is needed).
pub(crate) fn match_library<'p>(
    references: &[CitedReference],
    papers: &'p [Paper],
) -> Vec<Option<&'p Paper>> {
    let titled: Vec<(String, &Paper)> = papers
        .iter()
        .filter_map(|p| Some((padded(p.meta.title.as_deref()?)?, p)))
        .collect();
    references
        .iter()
        .map(|r| {
            let exact = r
                .title
                .as_deref()
                .and_then(padded)
                .and_then(|t| titled.iter().find(|(title, _)| *title == t));
            exact
                .or_else(|| {
                    let hay = format!(" {} ", normalize_title(&r.text));
                    titled
                        .iter()
                        .find(|(title, _)| hay.contains(title.as_str()))
                })
                .map(|(_, p)| *p)
        })
        .collect()
}

/// The normalized title space-padded, so containment in a padded haystack is
/// a whole-word match; `None` when too short to match safely.
fn padded(title: &str) -> Option<String> {
    let t = normalize_title(title);
    (t.len() >= MIN_TITLE_LEN).then(|| format!(" {t} "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::model::{Authors, PaperMeta, PaperStatus};

    fn paper(id: &str, title: Option<&str>) -> Paper {
        PaperMeta {
            title: title.map(Into::into),
            abstract_text: None,
            authors: Authors::default(),
            venue: None,
            year: None,
            doi: None,
            arxiv_id: None,
            dblp_key: None,
            url: None,
            source: None,
            status: PaperStatus::Resolved,
        }
        .into_paper(id.into(), format!("{id}.pdf"), None)
    }

    fn cited(text: &str, title: Option<&str>) -> CitedReference {
        CitedReference {
            text: text.into(),
            title: title.map(Into::into),
        }
    }

    fn library() -> Vec<Paper> {
        vec![
            paper("adam", Some("Adam: A Method for Stochastic Optimization")),
            paper(
                "resnet",
                Some("Deep Residual Learning for Image Recognition"),
            ),
            paper("untitled", None),
            paper("short", Some("On It")),
        ]
    }

    fn hashes(matches: &[Option<&Paper>]) -> Vec<Option<String>> {
        matches
            .iter()
            .map(|m| m.map(|p| p.content_hash.clone()))
            .collect()
    }

    #[test]
    fn a_title_spelled_inside_the_entry_matches() {
        let papers = library();
        let refs = [
            cited(
                "[12] D. Kingma, J. Ba. Adam: a method for stochastic optimization. ICLR 2015.",
                None,
            ),
            cited("[3] Some Unrelated Paper About Frogs. Nature 2001.", None),
            cited(
                "[1] A paper that mentions on it somewhere in prose. 2020.",
                None,
            ),
        ];
        assert_eq!(
            hashes(&match_library(&refs, &papers)),
            [Some("adam".into()), None, None]
        );
    }

    #[test]
    fn the_parsed_title_matches_when_the_raw_text_garbles_it() {
        let papers = library();
        let refs = [cited(
            "[7] Kingma+Ba, 3rd Intl Conf on Learning Repr, 2015.",
            Some("Adam: A Method for Stochastic Optimization"),
        )];
        assert_eq!(
            hashes(&match_library(&refs, &papers)),
            [Some("adam".into())]
        );
    }

    #[test]
    fn a_title_must_match_whole_words() {
        let papers = vec![paper("gat", Some("Graph Attention Network"))];
        let refs = [cited(
            "Knowledge Graph Attention Networks for recommendation",
            None,
        )];
        assert_eq!(hashes(&match_library(&refs, &papers)), [None]);
    }

    #[test]
    fn the_first_paper_wins_a_duplicate_title() {
        let papers = vec![
            paper("first", Some("Adam: A Method for Stochastic Optimization")),
            paper("second", Some("Adam: A Method for Stochastic Optimization")),
        ];
        let refs = [
            cited(
                "no title here",
                Some("Adam: A Method for Stochastic Optimization"),
            ),
            cited("Adam: A Method for Stochastic Optimization", None),
        ];
        assert_eq!(
            hashes(&match_library(&refs, &papers)),
            [Some("first".into()), Some("first".into())]
        );
    }

    #[test]
    fn titles_normalize_beyond_ascii() {
        let papers = vec![paper("de", Some("Über die Grundlagen der Mengenlehre"))];
        let refs = [cited(
            "E. Zermelo. ÜBER DIE GRUNDLAGEN DER MENGENLEHRE. 1908.",
            None,
        )];
        assert_eq!(hashes(&match_library(&refs, &papers)), [Some("de".into())]);
    }
}
