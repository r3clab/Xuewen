//! Page-aware chunking of `pdftotext` output for indexing and embedding.
//!
//! `seq 0` is a synthetic title+abstract chunk (strong paper-level semantic
//! target); body chunks are packed per page (never spanning a page, so
//! snippets can cite an exact page) to ~TARGET_CHARS with OVERLAP_CHARS of
//! carry-over between adjacent chunks.

const TARGET_CHARS: usize = 1200;
const OVERLAP_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chunk {
    pub(crate) seq: i64,
    /// 1-based PDF page; `None` for the synthetic seq-0 chunk.
    pub(crate) page: Option<i64>,
    pub(crate) text: String,
}

pub(crate) fn chunk_paper(
    title: Option<&str>,
    abstract_text: Option<&str>,
    body: &str,
) -> Vec<Chunk> {
    let mut out = Vec::new();
    let title = title.map(str::trim).filter(|s| !s.is_empty());
    let abstract_text = abstract_text.map(str::trim).filter(|s| !s.is_empty());
    let summary = match (title, abstract_text) {
        (Some(t), Some(a)) => Some(format!("{t}\n{a}")),
        (Some(t), None) => Some(t.to_string()),
        (None, Some(a)) => Some(a.to_string()),
        (None, None) => None,
    };
    if let Some(text) = summary {
        out.push(Chunk {
            seq: 0,
            page: None,
            text,
        });
    }
    let mut seq = 1;
    for (i, page) in body.split('\u{0c}').enumerate() {
        for text in chunk_page(page) {
            out.push(Chunk {
                seq,
                page: Some((i + 1) as i64),
                text,
            });
            seq += 1;
        }
    }
    out
}

/// Pack a page's paragraphs into chunks of ~TARGET_CHARS, carrying
/// OVERLAP_CHARS of tail text into the next chunk.
fn chunk_page(page: &str) -> Vec<String> {
    let paras: Vec<&str> = page
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let mut chunks: Vec<String> = Vec::new();
    let mut cur = String::new();
    for para in paras {
        for piece in split_long(para, TARGET_CHARS) {
            if !cur.is_empty() && cur.len() + piece.len() + 2 > TARGET_CHARS {
                let tail = overlap_tail(&cur, OVERLAP_CHARS);
                chunks.push(std::mem::take(&mut cur));
                cur = tail;
            }
            if !cur.is_empty() {
                cur.push_str("\n\n");
            }
            cur.push_str(&piece);
        }
    }
    if !cur.trim().is_empty() {
        chunks.push(cur);
    }
    chunks
}

/// Last ~`n` bytes of `s`, starting on a char boundary.
fn overlap_tail(s: &str, n: usize) -> String {
    let mut start = s.len().saturating_sub(n);
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    s[start..].trim_start().to_string()
}

/// Split a paragraph longer than `max` bytes, preferring sentence boundaries
/// (". "), hard-splitting on a char boundary as a last resort.
fn split_long(para: &str, max: usize) -> Vec<String> {
    if para.len() <= max {
        return vec![para.to_string()];
    }
    let mut out = Vec::new();
    let mut rest = para;
    while rest.len() > max {
        let mut window_end = max;
        while window_end < rest.len() && !rest.is_char_boundary(window_end) {
            window_end += 1;
        }
        let cut = match rest[..window_end].rfind(". ") {
            Some(i) if i > 0 => i + 1, // keep the period
            _ => window_end,
        };
        out.push(rest[..cut].trim().to_string());
        rest = rest[cut..].trim_start();
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq0_is_title_plus_abstract() {
        let out = chunk_paper(Some("A Title"), Some("An abstract."), "");
        assert_eq!(out[0].seq, 0);
        assert_eq!(out[0].page, None);
        assert_eq!(out[0].text, "A Title\nAn abstract.");
    }

    #[test]
    fn seq0_skipped_when_no_title_or_abstract() {
        let out = chunk_paper(None, None, "some body text");
        assert!(out.iter().all(|c| c.seq >= 1));
    }

    #[test]
    fn body_chunks_are_page_aware_and_sequential() {
        let body = "page one words\n\nmore text\u{0c}page two words";
        let out = chunk_paper(None, None, body);
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].seq, out[0].page), (1, Some(1)));
        assert_eq!((out[1].seq, out[1].page), (2, Some(2)));
        assert!(out[0].text.contains("page one words"));
        assert!(out[1].text.contains("page two words"));
    }

    #[test]
    fn long_page_splits_with_overlap() {
        // 5 paragraphs of ~400 chars force multiple chunks per page.
        let para = "x".repeat(395) + " end.";
        let body = vec![para.clone(); 5].join("\n\n");
        let out = chunk_paper(None, None, &body);
        assert!(
            out.len() >= 2,
            "expected multiple chunks, got {}",
            out.len()
        );
        for c in &out {
            assert!(c.text.len() <= TARGET_CHARS + OVERLAP_CHARS + 2);
        }
        // Overlap: the tail of chunk N reappears at the head of chunk N+1.
        // (overlap_tail is private but visible to this child test module.)
        let tail = overlap_tail(&out[0].text, OVERLAP_CHARS);
        assert!(
            out[1].text.starts_with(&tail),
            "chunk 2 must start with chunk 1's tail"
        );
    }

    #[test]
    fn paragraph_longer_than_target_is_split_at_sentences() {
        let sentence = "This sentence is exactly some words long. ";
        let para = sentence.repeat(60); // ~2500 chars, no blank lines
        let out = chunk_paper(None, None, &para);
        assert!(out.len() >= 2);
        assert!(out
            .iter()
            .all(|c| c.text.len() <= TARGET_CHARS + OVERLAP_CHARS + 2));
    }

    #[test]
    fn empty_body_yields_nothing() {
        assert!(chunk_paper(None, None, "").is_empty());
        assert!(chunk_paper(None, None, "\u{0c}\u{0c}").is_empty());
    }

    #[test]
    fn multibyte_text_never_panics() {
        let body = "日本語のテキスト。".repeat(400);
        let out = chunk_paper(Some("héllo"), None, &body);
        assert!(!out.is_empty()); // no panic on char boundaries
    }
}
