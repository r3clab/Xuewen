use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};

use crate::search::store::IndexRow;

/// The searchable identity of a paper, as seen by the staleness scan.
#[derive(Debug, Clone)]
pub(crate) struct PaperState {
    pub(crate) id: String,
    pub(crate) content_hash: String,
    pub(crate) meta_hash: String,
    /// Hash of the paper's annotation notes, which the FTS tier indexes.
    pub(crate) notes_hash: String,
    pub(crate) trashed: bool,
}

/// One paper's pending indexing work (at least one tier is true).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Work {
    pub(crate) paper_id: String,
    /// The Tantivy doc must be rewritten.
    pub(crate) fts: bool,
    /// The paper's text must be re-derived from its PDF (pdftotext + chunk).
    /// Implies `fts`; a note edit rewrites the doc from the stored chunks
    /// instead, which is the whole point of tracking a notes hash separately.
    pub(crate) reextract: bool,
    pub(crate) vectors: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Plan {
    pub(crate) index: Vec<Work>,
    /// Tombstones: index entries whose paper is trashed or gone.
    pub(crate) deindex: Vec<String>,
}

/// Per-tier staleness of one paper against its index row — the single
/// definition of "needs indexing" that both `plan` (with backoff gating on
/// top) and `SearchService::status` (counting, no backoff) consume.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TierStale {
    /// The Tantivy doc must be rewritten.
    pub(crate) fts: bool,
    /// The chunks themselves are stale: pdftotext + chunk again. Implies
    /// `fts`.
    reextract: bool,
    pub(crate) vectors: bool,
}

pub(crate) fn tier_staleness(
    p: &PaperState,
    row: Option<&IndexRow>,
    embed_model: Option<&str>,
) -> TierStale {
    let content_changed = row
        .map(|r| r.content_hash != p.content_hash || r.meta_hash != p.meta_hash)
        .unwrap_or(true);
    let notes_changed = row.map(|r| r.notes_hash != p.notes_hash).unwrap_or(true);
    // A missing FTS stamp still re-extracts: that is what makes `index
    // rebuild` the escape hatch when the chunker itself changes, since
    // nothing else ever re-derives the stored chunks.
    let reextract = content_changed || row.map(|r| r.fts_indexed_at.is_none()).unwrap_or(true);
    let fts = reextract || notes_changed;
    let vectors = embed_model.is_some()
        && (content_changed
            || row
                .map(|r| r.vectors_indexed_at.is_none() || r.embed_model.as_deref() != embed_model)
                .unwrap_or(true));
    TierStale {
        fts,
        reextract,
        vectors,
    }
}

/// Compare live papers against `search_index` and decide what to do.
/// Pure: all clock and IO inputs are parameters.
pub(crate) fn plan(
    papers: &[PaperState],
    rows: &[IndexRow],
    embed_model: Option<&str>,
    now: DateTime<Utc>,
) -> Plan {
    let by_id: HashMap<&str, &IndexRow> = rows.iter().map(|r| (r.paper_id.as_str(), r)).collect();
    let live: HashSet<&str> = papers
        .iter()
        .filter(|p| !p.trashed)
        .map(|p| p.id.as_str())
        .collect();
    let mut out = Plan::default();

    for p in papers.iter().filter(|p| !p.trashed) {
        let row = by_id.get(p.id.as_str()).copied();
        let stale = tier_staleness(p, row, embed_model);
        // Each tier waits out its OWN backoff: a broken embedder must not
        // delay a healthy FTS reindex, and vice versa.
        let fts = stale.fts
            && row.is_none_or(|r| {
                backoff_elapsed(r.fts_attempts, r.fts_last_attempt_at.as_deref(), now)
            });
        // Extraction is a shared precursor that rides with the FTS pass;
        // when the chunks themselves are stale but that pass is backed off,
        // the vector tier waits rather than embedding stale chunks on every
        // sweep.
        let vectors = stale.vectors
            && row.is_none_or(|r| {
                backoff_elapsed(r.vec_attempts, r.vec_last_attempt_at.as_deref(), now)
            })
            && (!stale.reextract || fts);
        if fts || vectors {
            out.index.push(Work {
                paper_id: p.id.clone(),
                fts,
                reextract: stale.reextract && fts,
                vectors,
            });
        }
    }
    for r in rows {
        if !live.contains(r.paper_id.as_str()) {
            out.deindex.push(r.paper_id.clone());
        }
    }
    out
}

/// After a failure, wait 60s · 2^(attempts−1), capped at one hour.
fn backoff_elapsed(attempts: i64, last_attempt_at: Option<&str>, now: DateTime<Utc>) -> bool {
    if attempts == 0 {
        return true;
    }
    let Some(last) = last_attempt_at.and_then(|s| DateTime::parse_from_rfc3339(s).ok()) else {
        return true;
    };
    let exp = (attempts - 1).clamp(0, 6) as u32;
    let wait = (60i64 << exp).min(3600);
    now.signed_duration_since(last.with_timezone(&Utc)) >= chrono::Duration::seconds(wait)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    fn ps(id: &str, ch: &str, mh: &str, trashed: bool) -> PaperState {
        PaperState {
            id: id.into(),
            content_hash: ch.into(),
            meta_hash: mh.into(),
            notes_hash: String::new(),
            trashed,
        }
    }

    fn row(id: &str, ch: &str, mh: &str) -> crate::search::store::IndexRow {
        crate::search::store::IndexRow {
            paper_id: id.into(),
            content_hash: ch.into(),
            meta_hash: mh.into(),
            notes_hash: String::new(),
            fts_indexed_at: Some("2026-07-09T00:00:00Z".into()),
            vectors_indexed_at: Some("2026-07-09T00:00:00Z".into()),
            embed_model: Some("m1".into()),
            fts_last_error: None,
            fts_attempts: 0,
            fts_last_attempt_at: None,
            vec_last_error: None,
            vec_attempts: 0,
            vec_last_attempt_at: None,
        }
    }

    #[test]
    fn new_paper_needs_both_tiers() {
        let p = plan(&[ps("a", "h", "m", false)], &[], Some("m1"), Utc::now());
        assert_eq!(p.index.len(), 1);
        assert!(p.index[0].fts && p.index[0].reextract && p.index[0].vectors);
        assert!(p.deindex.is_empty());
    }

    #[test]
    fn a_note_edit_rewrites_the_doc_without_re_extracting() {
        let mut p = ps("a", "h", "m", false);
        p.notes_hash = "n2".into();
        let mut r = row("a", "h", "m");
        r.notes_hash = "n1".into();
        let plan = plan(&[p], &[r], Some("m1"), Utc::now());
        assert_eq!(plan.index.len(), 1);
        let w = &plan.index[0];
        assert!(w.fts, "the Tantivy doc carries the notes");
        assert!(!w.reextract, "the PDF did not change — reuse stored chunks");
        assert!(!w.vectors, "nothing embeds notes");
    }

    #[test]
    fn matching_notes_are_not_work() {
        let mut p = ps("a", "h", "m", false);
        p.notes_hash = "n1".into();
        let mut r = row("a", "h", "m");
        r.notes_hash = "n1".into();
        assert!(plan(&[p], &[r], Some("m1"), Utc::now()).index.is_empty());
    }

    #[test]
    fn a_missing_fts_stamp_still_re_extracts() {
        // `index rebuild` clears the stamp; re-deriving the chunks is what
        // makes it the escape hatch when the chunker itself changes.
        let mut r = row("a", "h", "m");
        r.fts_indexed_at = None;
        let p = plan(&[ps("a", "h", "m", false)], &[r], None, Utc::now());
        assert!(p.index[0].fts && p.index[0].reextract);
    }

    #[test]
    fn up_to_date_paper_yields_no_work() {
        let p = plan(
            &[ps("a", "h", "m", false)],
            &[row("a", "h", "m")],
            Some("m1"),
            Utc::now(),
        );
        assert!(p.index.is_empty() && p.deindex.is_empty());
    }

    #[test]
    fn meta_change_and_content_change_force_both_tiers() {
        for (ch, mh) in [("h2", "m"), ("h", "m2")] {
            let p = plan(
                &[ps("a", ch, mh, false)],
                &[row("a", "h", "m")],
                Some("m1"),
                Utc::now(),
            );
            assert!(p.index[0].fts && p.index[0].vectors, "case ({ch},{mh})");
        }
    }

    #[test]
    fn model_change_re_embeds_without_touching_fts() {
        let p = plan(
            &[ps("a", "h", "m", false)],
            &[row("a", "h", "m")],
            Some("m2"),
            Utc::now(),
        );
        assert_eq!(p.index.len(), 1);
        assert!(!p.index[0].fts && !p.index[0].reextract && p.index[0].vectors);
    }

    #[test]
    fn no_embedder_means_no_vector_work() {
        let p = plan(&[ps("a", "h", "m", false)], &[], None, Utc::now());
        assert!(p.index[0].fts && !p.index[0].vectors);
    }

    #[test]
    fn trashed_and_missing_papers_become_deindex_tombstones() {
        let p = plan(
            &[ps("a", "h", "m", true)],
            &[row("a", "h", "m"), row("gone", "h", "m")],
            Some("m1"),
            Utc::now(),
        );
        assert!(p.index.is_empty());
        let mut d = p.deindex.clone();
        d.sort();
        assert_eq!(d, vec!["a".to_string(), "gone".to_string()]);
    }

    #[test]
    fn failed_rows_back_off_exponentially_capped_at_an_hour() {
        let mut r = row("a", "h", "m");
        r.fts_indexed_at = None;
        r.fts_attempts = 2; // wait = 60 * 2^(2-1) = 120s
        r.fts_last_attempt_at = Some((Utc::now() - Duration::seconds(30)).to_rfc3339());
        let p = plan(&[ps("a", "h", "m", false)], &[r.clone()], None, Utc::now());
        assert!(p.index.is_empty(), "still inside the backoff window");

        r.fts_last_attempt_at = Some((Utc::now() - Duration::seconds(180)).to_rfc3339());
        let p = plan(&[ps("a", "h", "m", false)], &[r.clone()], None, Utc::now());
        assert_eq!(p.index.len(), 1, "window elapsed");

        r.fts_attempts = 50; // cap: never wait more than 3600s
        r.fts_last_attempt_at = Some((Utc::now() - Duration::seconds(3700)).to_rfc3339());
        let p = plan(&[ps("a", "h", "m", false)], &[r], None, Utc::now());
        assert_eq!(p.index.len(), 1);
    }

    #[test]
    fn a_backed_off_vector_tier_does_not_delay_fts_work() {
        // Broken embedder (vec backoff running), then a note edit: the FTS
        // rewrite must be scheduled immediately, without the vector tier.
        let mut p = ps("a", "h", "m", false);
        p.notes_hash = "n2".into();
        let mut r = row("a", "h", "m");
        r.notes_hash = "n1".into();
        r.vectors_indexed_at = None;
        r.vec_attempts = 5;
        r.vec_last_attempt_at = Some(Utc::now().to_rfc3339());
        let plan = plan(&[p], &[r], Some("m1"), Utc::now());
        assert_eq!(plan.index.len(), 1);
        let w = &plan.index[0];
        assert!(w.fts && !w.reextract);
        assert!(!w.vectors, "vector tier is still inside its own backoff");
    }

    #[test]
    fn a_backed_off_fts_tier_does_not_delay_vector_work() {
        // Chunks are current (content matches, FTS stamp set); the FTS tier
        // is backed off from a notes-rewrite failure while the model changed:
        // the re-embed proceeds alone.
        let mut p = ps("a", "h", "m", false);
        p.notes_hash = "n2".into();
        let mut r = row("a", "h", "m");
        r.notes_hash = "n1".into();
        r.fts_attempts = 3;
        r.fts_last_attempt_at = Some(Utc::now().to_rfc3339());
        let plan = plan(&[p], &[r], Some("m2"), Utc::now());
        assert_eq!(plan.index.len(), 1);
        let w = &plan.index[0];
        assert!(!w.fts && !w.reextract && w.vectors);
    }

    #[test]
    fn vector_work_waits_for_a_backed_off_reextract() {
        // Content changed (the stored chunks are stale) but the FTS pass that
        // would re-derive them is backed off: schedule nothing rather than
        // embed stale chunks on every sweep.
        let mut r = row("a", "h", "m");
        r.fts_attempts = 2;
        r.fts_last_attempt_at = Some(Utc::now().to_rfc3339());
        let p = plan(&[ps("a", "h2", "m", false)], &[r], Some("m1"), Utc::now());
        assert!(p.index.is_empty());
    }
}
