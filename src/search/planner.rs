use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};

use crate::search::store::IndexRow;

/// The searchable identity of a paper, as seen by the staleness scan.
#[derive(Debug, Clone)]
pub(crate) struct PaperState {
    pub(crate) id: String,
    pub(crate) content_hash: String,
    pub(crate) summary_hash: String,
    pub(crate) doc_hash: String,
    pub(crate) trashed: bool,
}

/// How much of a paper's vector state must be rewritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VectorWork {
    None,
    /// Only the title + abstract changed: the seq-0 point alone.
    Summary,
    All,
}

/// One paper's pending indexing work (at least one tier has some).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Work {
    pub(crate) paper_id: String,
    /// The Tantivy doc must be rewritten.
    pub(crate) fts: bool,
    /// The body must be re-derived from the PDF (pdftotext + chunk). Implies
    /// `fts`; a metadata or note edit rewrites the doc from the stored
    /// chunks instead, since `pdftotext` is by far the most expensive step.
    pub(crate) reextract: bool,
    pub(crate) vectors: VectorWork,
}

#[derive(Debug, Default)]
pub(crate) struct Plan {
    pub(crate) index: Vec<Work>,
    /// Tombstones: index entries whose paper is trashed or gone.
    pub(crate) deindex: Vec<String>,
    /// When the earliest backed-off work falls due.
    pub(crate) retry_at: Option<DateTime<Utc>>,
}

/// Per-tier staleness of one paper against its index row — the single
/// definition of "needs indexing" that both `plan` (with backoff gating on
/// top) and `SearchService::status` (counting, no backoff) consume.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TierStale {
    pub(crate) fts: bool,
    reextract: bool,
    pub(crate) vectors: VectorWork,
}

pub(crate) fn tier_staleness(
    p: &PaperState,
    row: Option<&IndexRow>,
    embed_model: Option<&str>,
) -> TierStale {
    let content_changed = row.is_none_or(|r| r.content_hash != p.content_hash);
    // A missing FTS stamp still re-extracts: that is what makes `index
    // rebuild` the escape hatch when the chunker itself changes, since
    // nothing else ever re-derives the stored chunks.
    let reextract = content_changed || row.is_none_or(|r| r.fts_indexed_at.is_none());
    let fts = reextract || row.is_none_or(|r| r.doc_hash != p.doc_hash);
    let vectors = match (embed_model, row) {
        (None, _) => VectorWork::None,
        (Some(model), Some(r))
            if !content_changed
                && r.vectors_indexed_at.is_some()
                && r.embed_model.as_deref() == Some(model) =>
        {
            if r.summary_hash == p.summary_hash {
                VectorWork::None
            } else {
                VectorWork::Summary
            }
        }
        (Some(_), _) => VectorWork::All,
    };
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
    let mut due =
        |attempts: i64, last_attempt_at: Option<&str>| match retry_at(attempts, last_attempt_at) {
            Some(at) if at > now => {
                out.retry_at = Some(out.retry_at.map_or(at, |t| t.min(at)));
                false
            }
            _ => true,
        };

    let mut index = Vec::new();
    for p in papers.iter().filter(|p| !p.trashed) {
        let row = by_id.get(p.id.as_str()).copied();
        let stale = tier_staleness(p, row, embed_model);
        // Each tier waits out its OWN backoff: a broken embedder must not
        // delay a healthy FTS reindex, and vice versa.
        let fts =
            stale.fts && row.is_none_or(|r| due(r.fts_attempts, r.fts_last_attempt_at.as_deref()));
        // Extraction is a shared precursor that rides with the FTS pass;
        // when the chunks themselves are stale but that pass is backed off,
        // the vector tier waits rather than embedding stale chunks on every
        // sweep.
        let vectors_due = stale.vectors != VectorWork::None
            && row.is_none_or(|r| due(r.vec_attempts, r.vec_last_attempt_at.as_deref()))
            && (!stale.reextract || fts);
        let vectors = if vectors_due {
            stale.vectors
        } else {
            VectorWork::None
        };
        if fts || vectors != VectorWork::None {
            index.push(Work {
                paper_id: p.id.clone(),
                fts,
                reextract: stale.reextract && fts,
                vectors,
            });
        }
    }
    // A tombstone whose vector delete failed waits out the vector backoff;
    // its Tantivy doc goes right away regardless.
    let mut deindex = Vec::new();
    for r in rows.iter().filter(|r| !live.contains(r.paper_id.as_str())) {
        if r.fts_indexed_at.is_some() || due(r.vec_attempts, r.vec_last_attempt_at.as_deref()) {
            deindex.push(r.paper_id.clone());
        }
    }
    out.index = index;
    out.deindex = deindex;
    out
}

/// After a failure, wait 60s · 2^(attempts−1), capped at one hour.
fn retry_at(attempts: i64, last_attempt_at: Option<&str>) -> Option<DateTime<Utc>> {
    if attempts == 0 {
        return None;
    }
    let last = DateTime::parse_from_rfc3339(last_attempt_at?).ok()?;
    let exp = (attempts - 1).clamp(0, 6) as u32;
    let wait = (60i64 << exp).min(3600);
    Some(last.with_timezone(&Utc) + chrono::Duration::seconds(wait))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    fn ps(id: &str, ch: &str, sh: &str, trashed: bool) -> PaperState {
        PaperState {
            id: id.into(),
            content_hash: ch.into(),
            summary_hash: sh.into(),
            doc_hash: "d".into(),
            trashed,
        }
    }

    fn row(id: &str, ch: &str, sh: &str) -> IndexRow {
        IndexRow {
            paper_id: id.into(),
            content_hash: ch.into(),
            summary_hash: sh.into(),
            doc_hash: "d".into(),
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
        let p = plan(&[ps("a", "h", "s", false)], &[], Some("m1"), Utc::now());
        assert_eq!(p.index.len(), 1);
        let w = &p.index[0];
        assert!(w.fts && w.reextract);
        assert_eq!(w.vectors, VectorWork::All);
        assert!(p.deindex.is_empty() && p.retry_at.is_none());
    }

    #[test]
    fn a_doc_edit_rewrites_the_doc_without_re_extracting() {
        // Authors, venue or a note: only the Tantivy doc carries them.
        let mut p = ps("a", "h", "s", false);
        p.doc_hash = "d2".into();
        let plan = plan(&[p], &[row("a", "h", "s")], Some("m1"), Utc::now());
        assert_eq!(plan.index.len(), 1);
        let w = &plan.index[0];
        assert!(w.fts, "the Tantivy doc carries the edit");
        assert!(!w.reextract, "the PDF did not change — reuse stored chunks");
        assert_eq!(w.vectors, VectorWork::None, "nothing embeds it");
    }

    #[test]
    fn a_title_or_abstract_edit_re_embeds_only_the_summary() {
        let mut p = ps("a", "h", "s2", false);
        p.doc_hash = "d2".into();
        let plan = plan(&[p], &[row("a", "h", "s")], Some("m1"), Utc::now());
        let w = &plan.index[0];
        assert!(w.fts && !w.reextract);
        assert_eq!(w.vectors, VectorWork::Summary);
    }

    #[test]
    fn matching_hashes_are_not_work() {
        assert!(
            plan(
                &[ps("a", "h", "s", false)],
                &[row("a", "h", "s")],
                Some("m1"),
                Utc::now()
            )
            .index
            .is_empty()
        );
    }

    #[test]
    fn a_missing_fts_stamp_still_re_extracts() {
        // `index rebuild` clears the stamp; re-deriving the chunks is what
        // makes it the escape hatch when the chunker itself changes.
        let mut r = row("a", "h", "s");
        r.fts_indexed_at = None;
        let p = plan(&[ps("a", "h", "s", false)], &[r], None, Utc::now());
        assert!(p.index[0].fts && p.index[0].reextract);
    }

    #[test]
    fn a_content_change_re_extracts_and_re_embeds_everything() {
        let p = plan(
            &[ps("a", "h2", "s", false)],
            &[row("a", "h", "s")],
            Some("m1"),
            Utc::now(),
        );
        let w = &p.index[0];
        assert!(w.fts && w.reextract);
        assert_eq!(w.vectors, VectorWork::All);
    }

    #[test]
    fn model_change_re_embeds_without_touching_fts() {
        let p = plan(
            &[ps("a", "h", "s", false)],
            &[row("a", "h", "s")],
            Some("m2"),
            Utc::now(),
        );
        assert_eq!(p.index.len(), 1);
        let w = &p.index[0];
        assert!(!w.fts && !w.reextract);
        assert_eq!(w.vectors, VectorWork::All);
    }

    #[test]
    fn no_embedder_means_no_vector_work() {
        let p = plan(&[ps("a", "h", "s", false)], &[], None, Utc::now());
        assert!(p.index[0].fts);
        assert_eq!(p.index[0].vectors, VectorWork::None);
    }

    #[test]
    fn trashed_and_missing_papers_become_deindex_tombstones() {
        let p = plan(
            &[ps("a", "h", "s", true)],
            &[row("a", "h", "s"), row("gone", "h", "s")],
            Some("m1"),
            Utc::now(),
        );
        assert!(p.index.is_empty());
        let mut d = p.deindex.clone();
        d.sort();
        assert_eq!(d, vec!["a".to_string(), "gone".to_string()]);
    }

    #[test]
    fn a_tombstone_whose_vector_delete_failed_waits_out_the_backoff() {
        let now = Utc::now();
        let mut r = row("gone", "h", "s");
        r.fts_indexed_at = None;
        r.vec_attempts = 1;
        r.vec_last_attempt_at = Some((now - Duration::seconds(10)).to_rfc3339());
        let p = plan(&[], &[r.clone()], Some("m1"), now);
        assert!(p.deindex.is_empty());
        let retry = p.retry_at.expect("the delete is retried");
        assert!((retry - (now + Duration::seconds(50))).num_seconds().abs() <= 1);

        // A Tantivy doc still standing goes now, whatever the vector backoff.
        r.fts_indexed_at = Some("2026-07-09T00:00:00Z".into());
        assert_eq!(plan(&[], &[r], Some("m1"), now).deindex, vec!["gone"]);
    }

    #[test]
    fn failed_rows_back_off_exponentially_capped_at_an_hour() {
        let now = Utc::now();
        let mut r = row("a", "h", "s");
        r.fts_indexed_at = None;
        r.fts_attempts = 2; // wait = 60 * 2^(2-1) = 120s
        r.fts_last_attempt_at = Some((now - Duration::seconds(30)).to_rfc3339());
        let p = plan(&[ps("a", "h", "s", false)], &[r.clone()], None, now);
        assert!(p.index.is_empty(), "still inside the backoff window");
        let retry = p.retry_at.expect("the plan says when to come back");
        assert!((retry - (now + Duration::seconds(90))).num_seconds().abs() <= 1);

        r.fts_last_attempt_at = Some((now - Duration::seconds(180)).to_rfc3339());
        let p = plan(&[ps("a", "h", "s", false)], &[r.clone()], None, now);
        assert_eq!(p.index.len(), 1, "window elapsed");
        assert!(p.retry_at.is_none());

        r.fts_attempts = 50; // cap: never wait more than 3600s
        r.fts_last_attempt_at = Some((now - Duration::seconds(3700)).to_rfc3339());
        let p = plan(&[ps("a", "h", "s", false)], &[r], None, now);
        assert_eq!(p.index.len(), 1);
    }

    #[test]
    fn retry_at_is_the_earliest_deadline_of_stale_work_only() {
        let now = Utc::now();
        let backed_off = |id: &str, secs_ago: i64| {
            let mut r = row(id, "h", "s");
            r.fts_indexed_at = None;
            r.fts_attempts = 1;
            r.fts_last_attempt_at = Some((now - Duration::seconds(secs_ago)).to_rfc3339());
            r
        };
        // A current row's old failure state schedules nothing.
        let mut current = row("c", "h", "s");
        current.fts_attempts = 3;
        current.fts_last_attempt_at = Some(now.to_rfc3339());
        let papers = [
            ps("a", "h", "s", false),
            ps("b", "h", "s", false),
            ps("c", "h", "s", false),
        ];
        let p = plan(
            &papers,
            &[backed_off("a", 10), backed_off("b", 40), current],
            None,
            now,
        );
        assert!(p.index.is_empty());
        let retry = p.retry_at.unwrap();
        assert!((retry - (now + Duration::seconds(20))).num_seconds().abs() <= 1);
    }

    #[test]
    fn a_backed_off_vector_tier_does_not_delay_fts_work() {
        // Broken embedder (vec backoff running), then a note edit: the FTS
        // rewrite must be scheduled immediately, without the vector tier.
        let mut p = ps("a", "h", "s", false);
        p.doc_hash = "d2".into();
        let mut r = row("a", "h", "s");
        r.vectors_indexed_at = None;
        r.vec_attempts = 5;
        r.vec_last_attempt_at = Some(crate::db::now_rfc3339());
        let plan = plan(&[p], &[r], Some("m1"), Utc::now());
        assert_eq!(plan.index.len(), 1);
        let w = &plan.index[0];
        assert!(w.fts && !w.reextract);
        assert_eq!(
            w.vectors,
            VectorWork::None,
            "vector tier is still inside its own backoff"
        );
    }

    #[test]
    fn a_backed_off_fts_tier_does_not_delay_vector_work() {
        // Chunks are current (content matches, FTS stamp set); the FTS tier
        // is backed off from a doc-rewrite failure while the model changed:
        // the re-embed proceeds alone.
        let mut p = ps("a", "h", "s", false);
        p.doc_hash = "d2".into();
        let mut r = row("a", "h", "s");
        r.fts_attempts = 3;
        r.fts_last_attempt_at = Some(crate::db::now_rfc3339());
        let plan = plan(&[p], &[r], Some("m2"), Utc::now());
        assert_eq!(plan.index.len(), 1);
        let w = &plan.index[0];
        assert!(!w.fts && !w.reextract);
        assert_eq!(w.vectors, VectorWork::All);
    }

    #[test]
    fn vector_work_waits_for_a_backed_off_reextract() {
        // Content changed (the stored chunks are stale) but the FTS pass that
        // would re-derive them is backed off: schedule nothing rather than
        // embed stale chunks on every sweep.
        let mut r = row("a", "h", "s");
        r.fts_attempts = 2;
        r.fts_last_attempt_at = Some(crate::db::now_rfc3339());
        let p = plan(&[ps("a", "h2", "s", false)], &[r], Some("m1"), Utc::now());
        assert!(p.index.is_empty());
        assert!(p.retry_at.is_some());
    }
}
