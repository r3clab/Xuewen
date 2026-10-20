use super::naming;
use crate::library::model::Paper;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BibFormat {
    Bibtex,
    Biblatex,
}

/// One `.bib` entry for a paper (no trailing newline).
pub(crate) fn format_entry(p: &Paper, fmt: BibFormat) -> String {
    let kind = entry_type(p, fmt);
    let key = entry_key(p);
    let mut fields: Vec<(&'static str, String)> = Vec::new();

    if !p.meta.authors.0.is_empty() {
        fields.push(("author", p.meta.authors.0.join(" and ")));
    }
    if let Some(title) = p.meta.title.as_deref() {
        fields.push(("title", title.to_string()));
    }
    if let Some(venue) = p.meta.venue.as_deref() {
        fields.push((venue_field(kind, fmt), venue.to_string()));
    }
    if let Some(year) = p.meta.year {
        let field = if fmt == BibFormat::Biblatex {
            "date"
        } else {
            "year"
        };
        fields.push((field, year.to_string()));
    }
    if let Some(axv) = p.meta.arxiv_id.as_deref() {
        match fmt {
            BibFormat::Bibtex => fields.push(("archivePrefix", "arXiv".to_string())),
            BibFormat::Biblatex => fields.push(("eprinttype", "arxiv".to_string())),
        }
        fields.push(("eprint", axv.to_string()));
    }
    if let Some(doi) = p.meta.doi.as_deref() {
        fields.push(("doi", doi.to_string()));
    }
    if let Some(url) = entry_url(p) {
        fields.push(("url", url));
    }

    let mut out = format!("@{kind}{{{key},\n");
    for (name, value) in &fields {
        out.push_str(&format!("  {name} = {{{}}},\n", escape(value)));
    }
    out.push('}');
    out
}

/// Many entries, blank-line separated, with a single trailing newline.
/// An empty slice yields an empty string (not a bare newline).
pub(crate) fn format_entries(papers: &[Paper], fmt: BibFormat) -> String {
    if papers.is_empty() {
        return String::new();
    }
    let mut out = papers
        .iter()
        .map(|p| format_entry(p, fmt))
        .collect::<Vec<_>>()
        .join("\n\n");
    out.push('\n');
    out
}

fn entry_type(p: &Paper, fmt: BibFormat) -> &'static str {
    if let Some(key) = p.meta.dblp_key.as_deref() {
        if key.starts_with("conf/") {
            return "inproceedings";
        }
        if key.starts_with("journals/") {
            return "article";
        }
    }
    if p.meta.venue.is_some() {
        return "article";
    }
    if p.meta.arxiv_id.is_some() {
        return if fmt == BibFormat::Biblatex {
            "online"
        } else {
            "misc"
        };
    }
    "misc"
}

fn venue_field(kind: &str, fmt: BibFormat) -> &'static str {
    if kind == "inproceedings" {
        "booktitle"
    } else if fmt == BibFormat::Biblatex {
        "journaltitle"
    } else {
        "journal"
    }
}

fn entry_key(p: &Paper) -> String {
    if let Some(k) = p.cite_key.as_deref() {
        if !k.is_empty() {
            return k.to_string();
        }
    }
    if let (Some(first), Some(year)) = (p.meta.authors.0.first(), p.meta.year) {
        if let Some(s) = naming::surname(first) {
            let base = naming::fold_ascii_alnum(&format!("{s}{year}"));
            if !base.is_empty() {
                return base;
            }
        }
    }
    p.id.clone()
}

fn entry_url(p: &Paper) -> Option<String> {
    if let Some(u) = p.meta.url.as_deref() {
        if !u.is_empty() {
            return Some(u.to_string());
        }
    }
    p.meta
        .arxiv_id
        .as_deref()
        .map(|a| format!("https://arxiv.org/abs/{a}"))
}

/// Escape LaTeX-special characters in a field value.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '&' | '%' | '$' | '#' | '_' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::model::{Authors, PaperMeta, PaperStatus};

    fn paper() -> Paper {
        Paper {
            id: "01890000-0000-7000-8000-000000000001".into(),
            content_hash: "h".into(),
            rel_path: "h.pdf".into(),
            cite_key: Some("wang2019kgat".into()),
            added_at: "2026-07-09T00:00:00Z".into(),
            deleted_at: None,
            starred: false,
            name: None,
            meta: PaperMeta {
                title: Some("KGAT: Knowledge Graph Attention Network".into()),
                abstract_text: None,
                authors: Authors(vec!["Xiang Wang".into(), "Xiangnan He".into()]),
                venue: Some("KDD".into()),
                year: Some(2019),
                doi: Some("10.1145/3292500.3330701".into()),
                arxiv_id: None,
                dblp_key: Some("conf/kdd/WangHCLC19".into()),
                url: None,
                source: Some("dblp".into()),
                status: PaperStatus::Resolved,
            },
        }
    }

    #[test]
    fn inproceedings_from_dblp_conf_prefix() {
        let out = format_entry(&paper(), BibFormat::Bibtex);
        assert!(
            out.starts_with("@inproceedings{wang2019kgat,\n"),
            "got: {out}"
        );
        assert!(out.contains("author = {Xiang Wang and Xiangnan He},\n"));
        assert!(out.contains("title = {KGAT: Knowledge Graph Attention Network},\n"));
        assert!(out.contains("booktitle = {KDD},\n"));
        assert!(out.contains("year = {2019},\n"));
        assert!(out.contains("doi = {10.1145/3292500.3330701},\n"));
        assert!(out.ends_with("}"));
    }

    #[test]
    fn article_from_dblp_journals_prefix_and_biblatex_fields() {
        let mut p = paper();
        p.meta.dblp_key = Some("journals/tkde/Smith20".into());
        let out = format_entry(&p, BibFormat::Biblatex);
        assert!(out.starts_with("@article{wang2019kgat,\n"), "got: {out}");
        assert!(out.contains("journaltitle = {KDD},\n"));
        assert!(out.contains("date = {2019},\n"));
    }

    #[test]
    fn article_when_venue_but_no_dblp_key() {
        let mut p = paper();
        p.meta.dblp_key = None;
        let out = format_entry(&p, BibFormat::Bibtex);
        assert!(out.starts_with("@article{"), "got: {out}");
        assert!(out.contains("journal = {KDD},\n"));
    }

    #[test]
    fn arxiv_only_is_misc_bibtex_and_online_biblatex() {
        let mut p = paper();
        p.meta.dblp_key = None;
        p.meta.venue = None;
        p.meta.doi = None;
        p.meta.arxiv_id = Some("1706.03762".into());
        p.meta.url = None;

        let bt = format_entry(&p, BibFormat::Bibtex);
        assert!(bt.starts_with("@misc{"), "got: {bt}");
        assert!(bt.contains("archivePrefix = {arXiv},\n"));
        assert!(bt.contains("eprint = {1706.03762},\n"));
        assert!(bt.contains("url = {https://arxiv.org/abs/1706.03762},\n"));

        let bl = format_entry(&p, BibFormat::Biblatex);
        assert!(bl.starts_with("@online{"), "got: {bl}");
        assert!(bl.contains("eprinttype = {arxiv},\n"));
        assert!(bl.contains("eprint = {1706.03762},\n"));
    }

    #[test]
    fn escapes_latex_specials_and_omits_missing_fields() {
        let mut p = paper();
        p.meta.title = Some("Cost & Effect: 50% Faster #wins".into());
        p.meta.doi = None;
        let out = format_entry(&p, BibFormat::Bibtex);
        assert!(
            out.contains(r"title = {Cost \& Effect: 50\% Faster \#wins},"),
            "got: {out}"
        );
        assert!(!out.contains("doi ="));
    }

    #[test]
    fn key_falls_back_to_surname_year_then_id() {
        let mut p = paper();
        p.cite_key = None;
        assert!(format_entry(&p, BibFormat::Bibtex).starts_with("@inproceedings{wang2019,"));

        p.meta.authors = Authors(vec![]);
        p.meta.year = None;
        assert!(format_entry(&p, BibFormat::Bibtex)
            .starts_with("@inproceedings{01890000-0000-7000-8000-000000000001,"));
    }

    #[test]
    fn escapes_backslash_without_double_escaping() {
        let mut p = paper();
        p.meta.title = Some(r"A\B".into());
        let out = format_entry(&p, BibFormat::Bibtex);
        assert!(out.contains(r"title = {A\textbackslash{}B},"), "got: {out}");
    }

    #[test]
    fn stored_url_wins_over_arxiv_fallback() {
        let mut p = paper();
        p.meta.url = Some("https://example.com/paper".into());
        p.meta.arxiv_id = Some("1706.03762".into());
        let out = format_entry(&p, BibFormat::Bibtex);
        assert!(
            out.contains("url = {https://example.com/paper},"),
            "got: {out}"
        );
        assert!(
            !out.contains("https://arxiv.org/abs/1706.03762"),
            "got: {out}"
        );
    }

    #[test]
    fn batch_joins_entries_with_blank_line() {
        let out = format_entries(&[paper(), paper()], BibFormat::Bibtex);
        assert_eq!(out.matches("@inproceedings{").count(), 2);
        assert!(out.contains("}\n\n@inproceedings{"));
        assert!(out.ends_with("}\n"));
    }

    #[test]
    fn empty_batch_is_empty_string() {
        assert_eq!(format_entries(&[], BibFormat::Bibtex), "");
    }
}
