use anyhow::Result;
use sqlx::SqlitePool;

use crate::ingest::pipeline::Outcome;
use crate::library::papers;

/// Ask a yes/no question on the terminal; returns true only on an explicit yes.
pub(super) fn confirm(prompt: &str) -> anyhow::Result<bool> {
    use std::io::Write;
    print!("{prompt} [y/N] ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// One-line preview of quoted text, cut on a character boundary. A PDF
/// selection routinely spans line breaks, which would otherwise split one
/// annotation across several rows of the listing.
pub(super) fn ellipsize(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let head: String = flat.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", head.trim_end())
}

/// First three authors, "et al."-truncated — matches the web UI's rule.
pub(super) fn author_line(authors: &[String]) -> String {
    if authors.len() > 3 {
        format!("{}, et al.", authors[..3].join(", "))
    } else {
        authors.join(", ")
    }
}

/// Terminal output: drop <mark> tags and undo the snippet's HTML escaping.
pub(super) fn strip_snippet_html(s: &str) -> String {
    s.replace("<mark>", "")
        .replace("</mark>", "")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

pub(super) async fn print_outcome(pool: &SqlitePool, outcome: Outcome) -> Result<()> {
    let cite_key = match &outcome {
        Outcome::Duplicate(id) | Outcome::SameWork(id) => {
            papers::get(pool, id).await?.and_then(|p| p.cite_key)
        }
        Outcome::Ingested(_) | Outcome::InTrash(_) => None,
    };
    println!("{}", outcome_line(&outcome, cite_key.as_deref()));
    Ok(())
}

fn outcome_line(outcome: &Outcome, cite_key: Option<&str>) -> String {
    match (outcome, cite_key) {
        (Outcome::Ingested(id), _) => format!("ingested {id}"),
        (Outcome::Duplicate(id), Some(key)) => format!("duplicate of {key} ({id}), skipped"),
        (Outcome::Duplicate(id), None) => format!("duplicate ({id}), skipped"),
        (Outcome::SameWork(id), Some(key)) => format!("already in library as {key} ({id})"),
        (Outcome::SameWork(id), None) => format!("already in library ({id})"),
        (Outcome::InTrash(id), _) => format!("in trash — run: xuewen restore {id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{ellipsize, outcome_line};
    use crate::ingest::pipeline::Outcome;

    #[test]
    fn short_text_passes_through() {
        assert_eq!(ellipsize("a short quote", 60), "a short quote");
    }

    #[test]
    fn line_breaks_and_runs_of_space_collapse() {
        // PDF selections carry the newlines of the wrapped source text; the
        // listing is one row per annotation, so they cannot survive.
        assert_eq!(ellipsize("two\nlines   here", 60), "two lines here");
    }

    #[test]
    fn overlong_text_is_cut_with_an_ellipsis() {
        assert_eq!(ellipsize("abcdefghij", 5), "abcd…");
    }

    #[test]
    fn cutting_respects_character_boundaries() {
        // Byte slicing here would panic mid-codepoint: each of these is three
        // bytes wide, so `max` counts characters, not bytes.
        assert_eq!(ellipsize("學問學問學問", 3), "學問…");
        assert_eq!(ellipsize("學問學問學問", 4), "學問學…");
        assert_eq!(ellipsize("學問學問學問", 6), "學問學問學問");
    }

    #[test]
    fn same_work_names_the_existing_cite_key() {
        let outcome = Outcome::SameWork("p1".into());
        assert_eq!(
            outcome_line(&outcome, Some("he2016deep")),
            "already in library as he2016deep (p1)"
        );
        assert_eq!(outcome_line(&outcome, None), "already in library (p1)");
    }
}
