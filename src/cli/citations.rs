use anyhow::Result;

use super::output::{author_line, ellipsize, title};
use super::Ctx;
use crate::citations::StructuredReference;
use crate::library::papers;

#[derive(clap::Args)]
pub(super) struct CitationsArgs {
    /// Paper id (exact or unique prefix).
    paper: String,
}

pub(super) async fn citations(ctx: &Ctx, args: CitationsArgs) -> Result<()> {
    let paper = papers::find_one(&ctx.pool, &args.paper).await?;
    let Some(cached) = ctx.app.citations().cached(&paper.id).await? else {
        println!(
            "no parsed references yet — references are extracted when the paper is opened in the web reader"
        );
        return Ok(());
    };
    let parsed = cached.entries.iter().filter(|(_, p)| p.is_some()).count();
    println!(
        "{} references ({parsed} parsed; {}, {})",
        cached.entries.len(),
        cached.provenance,
        cached.created_at
    );
    for (i, (raw, reference)) in cached.entries.iter().enumerate() {
        let line = match reference {
            Some(r) => reference_line(r),
            None => format!("(unparsed) {}", ellipsize(raw, 100)),
        };
        println!("{:3}. {line}", i + 1);
    }
    Ok(())
}

fn reference_line(r: &StructuredReference) -> String {
    let mut head = author_line(&r.authors);
    if let Some(year) = r.year {
        if !head.is_empty() {
            head.push(' ');
        }
        head.push_str(&format!("({year})"));
    }
    let mut parts = vec![head];
    parts.push(title(r.title.as_deref()).to_string());
    parts.extend(r.venue.clone());
    parts.extend(r.doi.as_ref().map(|d| format!("doi:{d}")));
    parts.extend(r.arxiv_id.as_ref().map(|a| format!("arXiv:{a}")));
    if r.doi.is_none() && r.arxiv_id.is_none() {
        parts.extend(r.url.clone());
    }
    sentences(parts.iter().map(String::as_str))
}

/// Join fragments as sentences without doubling a period one already ends
/// with ("et al.", "Proc.").
fn sentences<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for part in parts.filter(|p| !p.is_empty()) {
        if !out.is_empty() {
            if !out.ends_with(['.', '?', '!']) {
                out.push('.');
            }
            out.push(' ');
        }
        out.push_str(part);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::sentences;

    #[test]
    fn sentences_never_double_a_period() {
        assert_eq!(
            sentences(["A, B, C, et al.", "Title?", "NeurIPS", ""].into_iter()),
            "A, B, C, et al. Title? NeurIPS"
        );
    }
}
