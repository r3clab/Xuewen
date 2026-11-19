use std::collections::HashMap;

use anyhow::Result;
use sha2::{Digest, Sha256};
use sqlx::{QueryBuilder, Sqlite, SqlitePool};

use crate::db::now_rfc3339;
use crate::library::model::Paper;
use crate::library::papers::{push_paper_filters, PaperFilter};
use crate::search::chunker::{self, Chunk};

/// State of a paper's derived search indexes. May outlive its paper (tombstone).
#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct IndexRow {
    pub(crate) paper_id: String,
    /// The PDF content the stored body chunks were extracted from.
    pub(crate) content_hash: String,
    /// `summary_hash` of the seq-0 text the vector tier last embedded.
    pub(crate) summary_hash: String,
    /// `doc_hash` of the metadata and notes in the Tantivy doc, stamped
    /// together with `fts_indexed_at`.
    pub(crate) doc_hash: String,
    pub(crate) fts_indexed_at: Option<String>,
    pub(crate) vectors_indexed_at: Option<String>,
    pub(crate) embed_model: Option<String>,
    /// Failure bookkeeping is per tier: the FTS and vector tiers fail
    /// independently, and one tier's backoff must neither delay nor be
    /// reset by the other tier's traffic.
    pub(crate) fts_last_error: Option<String>,
    pub(crate) fts_attempts: i64,
    pub(crate) fts_last_attempt_at: Option<String>,
    pub(crate) vec_last_error: Option<String>,
    pub(crate) vec_attempts: i64,
    pub(crate) vec_last_attempt_at: Option<String>,
}

/// Which tier an indexing failure charges backoff against. `Both` is for
/// failures before the tiers split (extraction/chunking): the broken
/// precursor serves both tiers, so neither may hot-loop on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErrorTier {
    Fts,
    Vectors,
    Both,
}

fn digest(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for part in parts {
        h.update(part.as_bytes());
        h.update([0x1f]);
    }
    hex::encode(h.finalize())
}

fn summary_digest(summary: Option<&str>) -> String {
    digest(&[summary.unwrap_or("")])
}

/// Hash of the seq-0 (title + abstract) text, the only metadata the vector
/// tier embeds. Comparing hashes against the stamps is how identify/refresh
/// edits are detected without any event plumbing in the mutation paths.
pub(crate) fn summary_hash(p: &Paper) -> String {
    summary_digest(
        chunker::summary_text(p.meta.title.as_deref(), p.meta.abstract_text.as_deref()).as_deref(),
    )
}

/// Hash of everything in a paper's Tantivy doc except the body, which
/// `content_hash` covers.
pub(crate) fn doc_hash(p: &Paper, notes: &str) -> String {
    digest(&[
        p.meta.title.as_deref().unwrap_or(""),
        &p.meta.authors.0.join(" ; "),
        p.meta.venue.as_deref().unwrap_or(""),
        p.meta.abstract_text.as_deref().unwrap_or(""),
        notes,
    ])
}

pub(crate) async fn all_index_rows(pool: &SqlitePool) -> Result<Vec<IndexRow>> {
    let rows = sqlx::query_as::<_, IndexRow>("SELECT * FROM search_index")
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Replace a paper's body chunks (seq ≥ 1) with ones extracted from
/// `content_hash`. The FTS stamp always clears, since the doc must be
/// rewritten from them; the vector stamp only when the body actually changed,
/// so re-extracting identical text re-embeds nothing. Returns whether it
/// changed. One transaction.
pub(crate) async fn replace_body(
    pool: &SqlitePool,
    paper_id: &str,
    chunks: &[Chunk],
    content_hash: &str,
) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let stored: Vec<(i64, Option<i64>, String)> = sqlx::query_as(
        "SELECT seq, page, text FROM chunks WHERE paper_id = ? AND seq >= 1 ORDER BY seq",
    )
    .bind(paper_id)
    .fetch_all(&mut *tx)
    .await?;
    let changed = !stored
        .into_iter()
        .map(|(seq, page, text)| Chunk { seq, page, text })
        .eq(chunks.iter().cloned());
    if changed {
        sqlx::query("DELETE FROM chunks WHERE paper_id = ? AND seq >= 1")
            .bind(paper_id)
            .execute(&mut *tx)
            .await?;
        for c in chunks {
            sqlx::query("INSERT INTO chunks (paper_id, seq, page, text) VALUES (?,?,?,?)")
                .bind(paper_id)
                .bind(c.seq)
                .bind(c.page)
                .bind(&c.text)
                .execute(&mut *tx)
                .await?;
        }
    }
    sqlx::query(
        "INSERT INTO search_index (paper_id, content_hash) VALUES (?, ?) \
         ON CONFLICT(paper_id) DO UPDATE SET \
           content_hash = excluded.content_hash, fts_indexed_at = NULL, \
           vectors_indexed_at = CASE WHEN ? THEN NULL ELSE vectors_indexed_at END",
    )
    .bind(paper_id)
    .bind(content_hash)
    .bind(changed)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(changed)
}

pub(crate) async fn body_chunks(pool: &SqlitePool, paper_id: &str) -> Result<Vec<Chunk>> {
    let rows: Vec<(i64, Option<i64>, String)> = sqlx::query_as(
        "SELECT seq, page, text FROM chunks WHERE paper_id = ? AND seq >= 1 ORDER BY seq",
    )
    .bind(paper_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(seq, page, text)| Chunk { seq, page, text })
        .collect())
}

/// The seq-0 text the vector tier last embedded.
pub(crate) async fn embedded_summary(pool: &SqlitePool, paper_id: &str) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT text FROM chunks WHERE paper_id = ? AND seq = 0")
            .bind(paper_id)
            .fetch_optional(pool)
            .await?,
    )
}

/// Chunk texts by `(paper_id, seq)`, in one query; missing chunks are absent.
pub(crate) async fn chunk_texts(
    pool: &SqlitePool,
    keys: &[(String, i64)],
) -> Result<HashMap<(String, i64), String>> {
    if keys.is_empty() {
        return Ok(HashMap::new());
    }
    let mut qb: QueryBuilder<Sqlite> =
        QueryBuilder::new("SELECT paper_id, seq, text FROM chunks WHERE (paper_id, seq) IN (");
    qb.push_values(keys, |mut row, (paper_id, seq)| {
        row.push_bind(paper_id).push_bind(seq);
    });
    qb.push(")");
    let rows: Vec<(String, i64, String)> = qb.build_query_as().fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(|(paper_id, seq, text)| ((paper_id, seq), text))
        .collect())
}

/// Stamp the FTS tier for docs a commit just made durable. Each `doc_hash`
/// is of what was actually written into the doc — not of whatever is in the
/// tables now — so an edit made mid-sweep still reads as stale on the next
/// pass. Resets only this tier's failure state: a successful FTS write must
/// not wipe the vector tier's backoff.
pub(crate) async fn mark_fts_done(pool: &SqlitePool, docs: &[(&str, &str)]) -> Result<()> {
    let now = now_rfc3339();
    let mut tx = pool.begin().await?;
    for (paper_id, doc_hash) in docs {
        sqlx::query(
            "UPDATE search_index SET fts_indexed_at = ?, doc_hash = ?, \
             fts_attempts = 0, fts_last_error = NULL WHERE paper_id = ?",
        )
        .bind(&now)
        .bind(doc_hash)
        .bind(paper_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// What a successful vector pass rewrote.
#[derive(Debug, Clone, Copy)]
pub(crate) enum VectorsDone<'a> {
    /// Every point, embedded with this model.
    All { model: &'a str },
    /// The seq-0 point alone.
    Summary,
}

/// Stamp the vector tier. `summary` is the seq-0 text now embedded; it is
/// stored as chunk 0 together with its hash, which is how a later pass knows
/// what the seq-0 point holds. Resets only this tier's failure state.
pub(crate) async fn mark_vectors_done(
    pool: &SqlitePool,
    paper_id: &str,
    summary: Option<&str>,
    done: VectorsDone<'_>,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    match summary {
        Some(text) => {
            sqlx::query(
                "INSERT INTO chunks (paper_id, seq, page, text) VALUES (?, 0, NULL, ?) \
                 ON CONFLICT(paper_id, seq) DO UPDATE SET text = excluded.text",
            )
            .bind(paper_id)
            .bind(text)
            .execute(&mut *tx)
            .await?;
        }
        None => {
            sqlx::query("DELETE FROM chunks WHERE paper_id = ? AND seq = 0")
                .bind(paper_id)
                .execute(&mut *tx)
                .await?;
        }
    }
    sqlx::query(
        "UPDATE search_index SET summary_hash = ?, vec_attempts = 0, vec_last_error = NULL \
         WHERE paper_id = ?",
    )
    .bind(summary_digest(summary))
    .bind(paper_id)
    .execute(&mut *tx)
    .await?;
    if let VectorsDone::All { model } = done {
        sqlx::query(
            "UPDATE search_index SET vectors_indexed_at = ?, embed_model = ? WHERE paper_id = ?",
        )
        .bind(now_rfc3339())
        .bind(model)
        .bind(paper_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Record an indexing failure against `tier`, upserting the row if one
/// doesn't exist yet (e.g. a brand-new paper whose PDF extraction fails
/// before `replace_body` ever runs). The placeholder empty hashes never match
/// a real paper's, so the planner still schedules the work once backoff
/// elapses.
pub(crate) async fn record_error(
    pool: &SqlitePool,
    paper_id: &str,
    msg: &str,
    tier: ErrorTier,
) -> Result<()> {
    let now = now_rfc3339();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO search_index (paper_id, content_hash) VALUES (?, '') \
         ON CONFLICT(paper_id) DO NOTHING",
    )
    .bind(paper_id)
    .execute(&mut *tx)
    .await?;
    if matches!(tier, ErrorTier::Fts | ErrorTier::Both) {
        sqlx::query(
            "UPDATE search_index SET fts_last_error = ?, fts_attempts = fts_attempts + 1, \
             fts_last_attempt_at = ? WHERE paper_id = ?",
        )
        .bind(msg)
        .bind(&now)
        .bind(paper_id)
        .execute(&mut *tx)
        .await?;
    }
    if matches!(tier, ErrorTier::Vectors | ErrorTier::Both) {
        sqlx::query(
            "UPDATE search_index SET vec_last_error = ?, vec_attempts = vec_attempts + 1, \
             vec_last_attempt_at = ? WHERE paper_id = ?",
        )
        .bind(msg)
        .bind(&now)
        .bind(paper_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Drop a paper's index row and chunks (used after de-indexing a tombstone).
pub(crate) async fn remove_index_entry(pool: &SqlitePool, paper_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM chunks WHERE paper_id = ?")
        .bind(paper_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM search_index WHERE paper_id = ?")
        .bind(paper_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Record that a paper's Tantivy doc is gone while its row stays.
pub(crate) async fn clear_fts_stamp(pool: &SqlitePool, paper_id: &str) -> Result<()> {
    sqlx::query("UPDATE search_index SET fts_indexed_at = NULL WHERE paper_id = ?")
        .bind(paper_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Force re-indexing of the given tier(s) for every paper (rebuild).
pub(crate) async fn clear_stamps(pool: &SqlitePool, fts: bool, vectors: bool) -> Result<()> {
    if fts {
        sqlx::query(
            "UPDATE search_index SET fts_indexed_at = NULL, fts_attempts = 0, \
             fts_last_error = NULL",
        )
        .execute(pool)
        .await?;
    }
    if vectors {
        sqlx::query(
            "UPDATE search_index SET vectors_indexed_at = NULL, vec_attempts = 0, \
             vec_last_error = NULL",
        )
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Ids of the non-trashed papers matching `filter` — resolved BEFORE the
/// engines run, so their top-K truncation happens on the filtered set
/// (qualifiers must narrow the ranking, not be applied after the cutoff).
pub(crate) async fn filtered_paper_ids(
    pool: &SqlitePool,
    filter: &PaperFilter,
) -> Result<Vec<String>> {
    let mut qb: QueryBuilder<sqlx::Sqlite> =
        QueryBuilder::new("SELECT id FROM papers WHERE deleted_at IS NULL");
    push_paper_filters(&mut qb, filter);
    let ids: Vec<(String,)> = qb.build_query_as().fetch_all(pool).await?;
    Ok(ids.into_iter().map(|(id,)| id).collect())
}

/// Hydrate papers by id, preserving the order of `ids` (fusion order). The
/// `deleted_at` check stays even though org filters are pushed into the
/// engines: Tantivy can hold docs for just-trashed papers until the next
/// sweep, and an unfiltered search has no id scope to exclude them.
pub(crate) async fn papers_by_ids_ordered(pool: &SqlitePool, ids: &[String]) -> Result<Vec<Paper>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut qb: QueryBuilder<sqlx::Sqlite> =
        QueryBuilder::new("SELECT * FROM papers WHERE deleted_at IS NULL AND id IN (");
    let mut sep = qb.separated(", ");
    for id in ids {
        sep.push_bind(id);
    }
    qb.push(")");
    let papers = qb.build_query_as::<Paper>().fetch_all(pool).await?;
    // Reorder to match `ids` (SQL IN gives no ordering guarantee).
    let mut by_id: std::collections::HashMap<String, Paper> =
        papers.into_iter().map(|p| (p.id.clone(), p)).collect();
    Ok(ids.iter().filter_map(|id| by_id.remove(id)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::model::{Authors, PaperMeta, PaperStatus};
    use crate::search::chunker::Chunk;

    fn paper(id: &str, hash: &str, title: &str) -> Paper {
        Paper {
            id: id.into(),
            content_hash: hash.into(),
            rel_path: format!("{hash}.pdf"),
            cite_key: None,
            added_at: "2026-07-09T00:00:00Z".into(),
            deleted_at: None,
            starred: false,
            name: None,
            meta: PaperMeta {
                title: Some(title.into()),
                abstract_text: None,
                authors: Authors::default(),
                venue: None,
                year: Some(2026),
                doi: None,
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: PaperStatus::Resolved,
            },
        }
    }

    fn body(texts: &[&str]) -> Vec<Chunk> {
        texts
            .iter()
            .enumerate()
            .map(|(i, t)| Chunk {
                seq: i as i64 + 1,
                page: Some(1),
                text: t.to_string(),
            })
            .collect()
    }

    async fn mark_all_done(pool: &SqlitePool, paper_id: &str) {
        mark_fts_done(pool, &[(paper_id, "d")]).await.unwrap();
        mark_vectors_done(pool, paper_id, Some("T"), VectorsDone::All { model: "m" })
            .await
            .unwrap();
    }

    #[test]
    fn each_hash_covers_only_what_its_tier_consumes() {
        let a = paper("p1", "h1", "Title One");
        let edited = |f: fn(&mut Paper)| {
            let mut b = a.clone();
            f(&mut b);
            b
        };
        let retitled = edited(|p| p.meta.title = Some("Title Two".into()));
        let reauthored = edited(|p| p.meta.authors = Authors(vec!["Ada Lovelace".into()]));
        let revenued = edited(|p| p.meta.venue = Some("KDD".into()));
        let redated = edited(|p| p.meta.year = Some(1999));

        assert_ne!(summary_hash(&a), summary_hash(&retitled));
        for same in [&reauthored, &revenued, &redated] {
            assert_eq!(summary_hash(&a), summary_hash(same));
        }
        for changed in [&retitled, &reauthored, &revenued] {
            assert_ne!(doc_hash(&a, ""), doc_hash(changed, ""));
        }
        assert_eq!(
            doc_hash(&a, ""),
            doc_hash(&redated, ""),
            "year is indexed nowhere"
        );
        assert_ne!(doc_hash(&a, ""), doc_hash(&a, "a note"));
    }

    #[tokio::test]
    async fn replace_body_roundtrip_and_stamp_lifecycle() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("p1", "h1", "T"))
            .await
            .unwrap();

        assert!(replace_body(&pool, "p1", &body(&["one", "two"]), "h1")
            .await
            .unwrap());
        assert_eq!(
            body_chunks(&pool, "p1").await.unwrap(),
            body(&["one", "two"])
        );
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert!(row.fts_indexed_at.is_none() && row.vectors_indexed_at.is_none());

        mark_all_done(&pool, "p1").await;
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert!(row.fts_indexed_at.is_some() && row.vectors_indexed_at.is_some());
        assert_eq!(row.embed_model.as_deref(), Some("m"));
        assert_eq!(row.doc_hash, "d");
        assert_eq!(row.summary_hash, summary_digest(Some("T")));
        assert_eq!(
            embedded_summary(&pool, "p1").await.unwrap().as_deref(),
            Some("T")
        );
        assert_eq!(
            body_chunks(&pool, "p1").await.unwrap().len(),
            2,
            "chunk 0 is not body"
        );

        // Re-extracting the same text rewrites the doc but keeps the vectors.
        assert!(!replace_body(&pool, "p1", &body(&["one", "two"]), "h2")
            .await
            .unwrap());
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert!(row.fts_indexed_at.is_none() && row.vectors_indexed_at.is_some());
        assert_eq!(row.content_hash, "h2");

        // A different body invalidates both; chunk 0 belongs to the vector tier.
        mark_all_done(&pool, "p1").await;
        assert!(replace_body(&pool, "p1", &body(&["three"]), "h3")
            .await
            .unwrap());
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert!(row.fts_indexed_at.is_none() && row.vectors_indexed_at.is_none());
        assert_eq!(body_chunks(&pool, "p1").await.unwrap(), body(&["three"]));
        assert_eq!(
            embedded_summary(&pool, "p1").await.unwrap().as_deref(),
            Some("T")
        );
    }

    #[tokio::test]
    async fn a_summary_only_pass_stamps_the_summary_and_nothing_else() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("p1", "h1", "T"))
            .await
            .unwrap();
        replace_body(&pool, "p1", &body(&["one"]), "h1")
            .await
            .unwrap();
        record_error(&pool, "p1", "embed down", ErrorTier::Vectors)
            .await
            .unwrap();

        mark_vectors_done(&pool, "p1", Some("New"), VectorsDone::Summary)
            .await
            .unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert_eq!(row.summary_hash, summary_digest(Some("New")));
        assert!(
            row.vectors_indexed_at.is_none(),
            "the body was never embedded"
        );
        assert_eq!(row.vec_attempts, 0);
        assert_eq!(
            embedded_summary(&pool, "p1").await.unwrap().as_deref(),
            Some("New")
        );

        mark_vectors_done(&pool, "p1", None, VectorsDone::Summary)
            .await
            .unwrap();
        assert_eq!(embedded_summary(&pool, "p1").await.unwrap(), None);
        assert_eq!(body_chunks(&pool, "p1").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn chunk_texts_fetches_every_requested_chunk_that_exists() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        for id in ["p1", "p2"] {
            crate::library::papers::insert(&pool, &paper(id, id, "T"))
                .await
                .unwrap();
            replace_body(
                &pool,
                id,
                &body(&[&format!("{id} one"), &format!("{id} two")]),
                id,
            )
            .await
            .unwrap();
        }
        let keys = [
            ("p1".to_string(), 2),
            ("p2".to_string(), 1),
            ("p2".to_string(), 9),
        ];
        let got = chunk_texts(&pool, &keys).await.unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[&("p1".to_string(), 2)], "p1 two");
        assert_eq!(got[&("p2".to_string(), 1)], "p2 one");
        assert!(chunk_texts(&pool, &[]).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn record_error_increments_only_its_tier_and_marks_reset_only_theirs() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("p1", "h1", "T"))
            .await
            .unwrap();
        replace_body(&pool, "p1", &body(&["one"]), "h1")
            .await
            .unwrap();

        record_error(&pool, "p1", "boom", ErrorTier::Fts)
            .await
            .unwrap();
        record_error(&pool, "p1", "boom2", ErrorTier::Fts)
            .await
            .unwrap();
        record_error(&pool, "p1", "embed down", ErrorTier::Vectors)
            .await
            .unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert_eq!(row.fts_attempts, 2);
        assert_eq!(row.fts_last_error.as_deref(), Some("boom2"));
        assert!(row.fts_last_attempt_at.is_some());
        assert_eq!(row.vec_attempts, 1);
        assert_eq!(row.vec_last_error.as_deref(), Some("embed down"));

        // A successful FTS write resets only the FTS tier's backoff: the
        // broken embedder must keep backing off, not retry every sweep.
        mark_fts_done(&pool, &[("p1", "d")]).await.unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert_eq!(row.fts_attempts, 0);
        assert!(row.fts_last_error.is_none());
        assert_eq!(row.vec_attempts, 1);
        assert_eq!(row.vec_last_error.as_deref(), Some("embed down"));

        mark_vectors_done(&pool, "p1", None, VectorsDone::All { model: "m" })
            .await
            .unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert_eq!(row.vec_attempts, 0);
        assert!(row.vec_last_error.is_none());
    }

    #[tokio::test]
    async fn record_error_without_prior_row_creates_one() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        let p = paper("p1", "h1", "T");
        crate::library::papers::insert(&pool, &p).await.unwrap();
        // No replace_chunks call: no search_index row exists yet (mirrors a
        // brand-new paper whose PDF extraction fails before chunking).
        assert!(all_index_rows(&pool).await.unwrap().is_empty());

        record_error(&pool, "p1", "extraction failed", ErrorTier::Both)
            .await
            .unwrap();
        let rows = all_index_rows(&pool).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].fts_attempts, 1);
        assert_eq!(rows[0].vec_attempts, 1);
        assert_eq!(rows[0].fts_last_error.as_deref(), Some("extraction failed"));
        assert_eq!(rows[0].vec_last_error.as_deref(), Some("extraction failed"));
        assert_eq!(rows[0].content_hash, "");
        assert_eq!(rows[0].summary_hash, "");

        record_error(&pool, "p1", "extraction failed again", ErrorTier::Both)
            .await
            .unwrap();
        let rows = all_index_rows(&pool).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].fts_attempts, 2);
        assert_eq!(rows[0].vec_attempts, 2);
        assert_eq!(
            rows[0].fts_last_error.as_deref(),
            Some("extraction failed again")
        );
    }

    #[tokio::test]
    async fn remove_and_clear_stamps() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("p1", "h1", "T"))
            .await
            .unwrap();
        replace_body(&pool, "p1", &body(&["one"]), "h1")
            .await
            .unwrap();
        mark_all_done(&pool, "p1").await;
        record_error(&pool, "p1", "boom", ErrorTier::Both)
            .await
            .unwrap();

        clear_stamps(&pool, false, true).await.unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert!(row.fts_indexed_at.is_some() && row.vectors_indexed_at.is_none());
        // Per-tier reset: clearing the vector stamps leaves the FTS tier's
        // failure state alone.
        assert_eq!(row.vec_attempts, 0);
        assert!(row.vec_last_error.is_none());
        assert_eq!(row.fts_attempts, 1);
        assert!(row.fts_last_error.is_some());

        remove_index_entry(&pool, "p1").await.unwrap();
        assert!(all_index_rows(&pool).await.unwrap().is_empty());
        assert!(body_chunks(&pool, "p1").await.unwrap().is_empty());
        assert_eq!(embedded_summary(&pool, "p1").await.unwrap(), None);
    }

    #[tokio::test]
    async fn papers_by_ids_ordered_preserves_order_and_drops_trashed() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        for (id, hash, title) in [("a", "h1", "A"), ("b", "h2", "B"), ("c", "h3", "C")] {
            crate::library::papers::insert(&pool, &paper(id, hash, title))
                .await
                .unwrap();
        }
        crate::library::trash::soft_delete(&pool, "c")
            .await
            .unwrap();

        let ids = vec![
            "c".to_string(),
            "b".to_string(),
            "a".to_string(),
            "zz".to_string(),
        ];
        let got = papers_by_ids_ordered(&pool, &ids).await.unwrap();
        let got_ids: Vec<&str> = got.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(got_ids, vec!["b", "a"]); // trashed + unknown dropped, order kept
    }

    #[tokio::test]
    async fn filtered_paper_ids_applies_tag_prefix_starred_and_trash() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("a", "h1", "A"))
            .await
            .unwrap();
        crate::library::papers::insert(&pool, &paper("b", "h2", "B"))
            .await
            .unwrap();
        crate::library::papers::insert(&pool, &paper("c", "h3", "C"))
            .await
            .unwrap();
        crate::library::tags::add_paper(
            &pool,
            "a",
            &crate::library::tags::TagName::parse("security/fuzzing").unwrap(),
        )
        .await
        .unwrap();
        crate::library::papers::set_starred(&pool, "b", true)
            .await
            .unwrap();
        crate::library::trash::soft_delete(&pool, "c")
            .await
            .unwrap();

        let with_tag = |tag: &str| PaperFilter {
            tag: Some(tag.to_string()),
            ..Default::default()
        };

        let got = filtered_paper_ids(&pool, &with_tag("security"))
            .await
            .unwrap();
        assert_eq!(got, vec!["a".to_string()]);

        assert!(filtered_paper_ids(&pool, &with_tag("ml"))
            .await
            .unwrap()
            .is_empty());

        let starred_only = PaperFilter {
            starred: Some(true),
            ..Default::default()
        };
        let got = filtered_paper_ids(&pool, &starred_only).await.unwrap();
        assert_eq!(got, vec!["b".to_string()]);

        // No restrictions: every live paper, never the trashed one.
        let mut got = filtered_paper_ids(&pool, &PaperFilter::default())
            .await
            .unwrap();
        got.sort();
        assert_eq!(got, vec!["a".to_string(), "b".to_string()]);
    }
}
