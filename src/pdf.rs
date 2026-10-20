use anyhow::{anyhow, Result};
use regex::Regex;
use std::path::Path;
use std::process::Command;

/// `pdftotext` ran and exited non-zero: the input itself is unreadable, and
/// re-running on the same bytes is deterministic — callers with a retry
/// policy (the watcher) downcast for this and quarantine immediately. The
/// failed-to-*spawn* case stays an untyped error on purpose: a missing binary
/// is environmental and may well be fixed between retries.
#[derive(Debug)]
pub(crate) struct PdftotextFailed {
    stderr: String,
}

impl std::fmt::Display for PdftotextFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pdftotext failed: {}", self.stderr)
    }
}

impl std::error::Error for PdftotextFailed {}

/// Run `pdftotext <extra-args…> <path> -` and return stdout.
fn run_pdftotext(path: &Path, extra_args: &[&str]) -> Result<String> {
    let out = Command::new("pdftotext")
        .args(extra_args)
        .arg(path)
        .arg("-") // write to stdout
        .output()
        .map_err(|e| anyhow!("failed to run pdftotext (is poppler-utils installed?): {e}"))?;
    if !out.status.success() {
        return Err(PdftotextFailed {
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
        .into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Extract text from pages 1..=`last_page` using the `pdftotext` binary.
pub(crate) fn extract_text(path: &Path, last_page: u32) -> Result<String> {
    run_pdftotext(path, &["-f", "1", "-l", &last_page.to_string()])
}

/// Extract text from the whole document (no page limit), pages separated by
/// form feeds (`\f`), using the `pdftotext` binary.
pub(crate) fn extract_text_all(path: &Path) -> Result<String> {
    run_pdftotext(path, &[])
}

/// Repair pdftotext's small-caps artifact for a paper's own name: a word
/// rendered in small caps (e.g. "RTCᴏɴ") extracts as an uppercase pair
/// ("RTC ON"), which then pollutes anything derived from the text (LLM
/// summaries parrot "RTC ON"). For every distinctive token of the resolved
/// title (≥4 chars, ≥2 uppercase), rejoin two-piece uppercase splits of that
/// token — both pieces ≥2 chars so ordinary words are never glued together.
pub(crate) fn repair_smallcaps(text: &str, title: &str) -> String {
    let mut out = text.to_string();
    for token in title.split(|c: char| !c.is_ascii_alphanumeric()) {
        if token.len() < 4 || token.chars().filter(|c| c.is_ascii_uppercase()).count() < 2 {
            continue;
        }
        // One alternation over every valid split — a single compile and a
        // single scan per token instead of one per split point. (Token bytes
        // are ASCII alphanumeric by the split above, so byte slicing is safe.)
        let upper = token.to_ascii_uppercase();
        let splits: Vec<String> = (2..=upper.len() - 2)
            .map(|i| regex::escape(&format!("{} {}", &upper[..i], &upper[i..])))
            .collect();
        // \b on both ends: only whole-word pairs, never inside words.
        let re = Regex::new(&format!(r"\b(?:{})\b", splits.join("|")))
            .expect("escaped literals are a valid regex");
        // replace_all borrows when nothing matched (the common case for a
        // full-document `out`); only take the copy when it actually changed.
        if let std::borrow::Cow::Owned(s) = re.replace_all(&out, token) {
            out = s;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use printpdf::*;

    /// A single-page PDF page carrying one Helvetica line near the top.
    fn text_page(line: &str) -> PdfPage {
        let ops = vec![
            Op::StartTextSection,
            Op::SetFont {
                font: PdfFontHandle::Builtin(BuiltinFont::Helvetica),
                size: Pt(12.0),
            },
            Op::SetTextCursor {
                pos: Point::new(Mm(15.0), Mm(280.0)),
            },
            Op::ShowText {
                items: vec![TextItem::Text(line.to_string())],
            },
            Op::EndTextSection,
        ];
        PdfPage::new(Mm(210.0), Mm(297.0), ops)
    }

    fn write_pdf(path: &Path, line: &str) {
        let mut doc = PdfDocument::new("t");
        let bytes = doc
            .with_pages(vec![text_page(line)])
            .save(&PdfSaveOptions::default(), &mut Vec::new());
        std::fs::write(path, bytes).unwrap();
    }

    fn write_two_page_pdf(path: &Path, line1: &str, line2: &str) {
        let mut doc = PdfDocument::new("t");
        let bytes = doc
            .with_pages(vec![text_page(line1), text_page(line2)])
            .save(&PdfSaveOptions::default(), &mut Vec::new());
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn extracts_known_text() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("sample.pdf");
        write_pdf(&pdf, "Attention Is All You Need");
        let text = extract_text(&pdf, 1).unwrap();
        assert!(
            text.contains("Attention Is All You Need"),
            "extracted text was: {text:?}"
        );
    }

    #[test]
    fn extract_text_all_returns_every_page_with_separators() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("two.pdf");
        write_two_page_pdf(&pdf, "First Page Words", "Second Page Words");
        let text = extract_text_all(&pdf).unwrap();
        assert!(text.contains("First Page Words"));
        assert!(text.contains("Second Page Words"));
        assert!(text.contains('\u{0c}'), "pdftotext page separator expected");
    }

    #[test]
    fn repairs_smallcaps_splits_of_title_tokens() {
        let title = "RTCON: Context-Adaptive Function-Level Fuzzing for RTOS Kernels";
        let text = "we present RTC ON , a fuzzer. RTC ON employs classification.";
        assert_eq!(
            super::repair_smallcaps(text, title),
            "we present RTCON , a fuzzer. RTCON employs classification."
        );
    }

    #[test]
    fn repair_smallcaps_leaves_unrelated_text_alone() {
        let title = "RTCON: Context-Adaptive Fuzzing";
        let text = "THE CAT sat on RTOS mats"; // no split of any title token
        assert_eq!(super::repair_smallcaps(text, title), text);
    }

    #[test]
    fn repair_smallcaps_requires_two_chars_per_piece() {
        // "B ERT"/"BER T" (1-char piece) must NOT be rejoined; "BE RT" is.
        let title = "BERT: Pre-training of Deep Bidirectional Transformers";
        assert_eq!(
            super::repair_smallcaps("uses BE RT daily", title),
            "uses BERT daily"
        );
        assert_eq!(
            super::repair_smallcaps("plan B ERT now", title),
            "plan B ERT now"
        );
    }

    #[test]
    fn repair_smallcaps_with_empty_title_is_identity() {
        assert_eq!(super::repair_smallcaps("any text", ""), "any text");
    }

    #[test]
    fn corrupt_pdf_error_downcasts_to_pdftotext_failed() {
        // The watcher's quarantine fast path depends on this downcast.
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.pdf");
        std::fs::write(&bad, b"this is not a pdf").unwrap();
        let err = extract_text(&bad, 1).unwrap_err();
        assert!(
            err.chain()
                .any(|c| c.downcast_ref::<PdftotextFailed>().is_some()),
            "got: {err:#}"
        );
    }
}
