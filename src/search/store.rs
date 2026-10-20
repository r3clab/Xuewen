use anyhow::Result;
use sha2::{Digest, Sha256};
use sqlx::{QueryBuilder, SqlitePool};

use crate::db::now_rfc3339;
use crate::library::model::Paper;
use crate::library::papers::{push_paper_filters, PaperFilter};
use crate::search::chunker::Chunk;

/// State of a paper's derived search indexes. May outlive its paper (tombstone).
#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct IndexRow {
    pub(crate) paper_id: String,
    pub(crate) content_hash: String,
    pub(crate) meta_hash: String,
    /// Hash of the annotation notes that went into the Tantivy doc, stamped
    /// together with `fts_indexed_at`. Only the FTS tier carries notes.
    pub(crate) notes_hash: String,
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

/// Hash of the metadata that feeds the search indexes. Comparing this against
/// the stored value is how identify/refresh edits are detected without any
/// event plumbing in the mutation paths.
pub(crate) fn meta_hash(p: &Paper) -> String {
    let mut h = Sha256::new();
    for part in [
        p.meta.title.as_deref().unwrap_or(""),
        p.meta.abstract_text.as_deref().unwrap_or(""),
        p.meta.venue.as_deref().unwrap_or(""),
    ] {
        h.update(part.as_bytes());
        h.update([0x1f]);
    }
    h.update(
        p.meta
            .year
            .map(|y| y.to_string())
            .unwrap_or_default()
            .as_bytes(),
    );
    h.update([0x1f]);
    h.update(
        serde_json::to_string(&p.meta.authors)
            .unwrap_or_default()
            .as_bytes(),
    );
    hex::encode(h.finalize())
}

/// Hash of a paper's joined annotation notes. Empty in, empty out: "no notes"
/// is the empty stamp, which is also the column default, so a paper nobody has
/// annotated never looks stale.
pub(crate) fn notes_hash(blob: &str) -> String {
    if blob.is_empty() {
        return String::new();
    }
    hex::encode(Sha256::digest(blob.as_bytes()))
}

pub(crate) async fn all_index_rows(pool: &SqlitePool) -> Result<Vec<IndexRow>> {
    let rows = sqlx::query_as::<_, IndexRow>("SELECT * FROM search_index")
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Replace a paper's chunks and reset its index row (stamps cleared: both
/// tiers must re-index the new content). One transaction.
pub(crate) async fn replace_chunks(
    pool: &SqlitePool,
    paper_id: &str,
    chunks: &[Chunk],
    content_hash: &str,
    meta_hash: &str,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM chunks WHERE paper_id = ?")
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
    sqlx::query(
        "INSERT INTO search_index (paper_id, content_hash, meta_hash, chunk_count) \
         VALUES (?,?,?,?) \
         ON CONFLICT(paper_id) DO UPDATE SET \
           content_hash = excluded.content_hash, meta_hash = excluded.meta_hash, \
           chunk_count = excluded.chunk_count, \
           fts_indexed_at = NULL, vectors_indexed_at = NULL",
    )
    .bind(paper_id)
    .bind(content_hash)
    .bind(meta_hash)
    .bind(chunks.len() as i64)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn chunks_for_paper(pool: &SqlitePool, paper_id: &str) -> Result<Vec<Chunk>> {
    let rows: Vec<(i64, Option<i64>, String)> =
        sqlx::query_as("SELECT seq, page, text FROM chunks WHERE paper_id = ? ORDER BY seq")
            .bind(paper_id)
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(seq, page, text)| Chunk { seq, page, text })
        .collect())
}

pub(crate) async fn chunk_text(
    pool: &SqlitePool,
    paper_id: &str,
    seq: i64,
) -> Result<Option<Chunk>> {
    let row: Option<(i64, Option<i64>, String)> =
        sqlx::query_as("SELECT seq, page, text FROM chunks WHERE paper_id = ? AND seq = ?")
            .bind(paper_id)
            .bind(seq)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(seq, page, text)| Chunk { seq, page, text }))
}

/// Stamp the FTS tier. `notes_hash` is the hash of the notes blob that was
/// actually written into the Tantivy doc — not of whatever is in the table
/// now — so a note edited mid-sweep still reads as stale on the next pass.
/// Resets only this tier's failure state: a successful FTS write must not
/// wipe the vector tier's backoff.
pub(crate) async fn mark_fts_done(
    pool: &SqlitePool,
    paper_id: &str,
    notes_hash: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE search_index SET fts_indexed_at = ?, notes_hash = ?, \
         fts_attempts = 0, fts_last_error = NULL WHERE paper_id = ?",
    )
    .bind(now_rfc3339())
    .bind(notes_hash)
    .bind(paper_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn mark_vectors_done(
    pool: &SqlitePool,
    paper_id: &str,
    model: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE search_index SET vectors_indexed_at = ?, embed_model = ?, \
         vec_attempts = 0, vec_last_error = NULL WHERE paper_id = ?",
    )
    .bind(now_rfc3339())
    .bind(model)
    .bind(paper_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Record an indexing failure against `tier`, upserting the row if one
/// doesn't exist yet (e.g. a brand-new paper whose PDF extraction fails
/// before `replace_chunks` ever runs). The placeholder empty hashes never
/// match a real paper's content/meta hash, so the planner still schedules
/// the work once backoff elapses.
pub(crate) async fn record_error(
    pool: &SqlitePool,
    paper_id: &str,
    msg: &str,
    tier: ErrorTier,
) -> Result<()> {
    let now = now_rfc3339();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO search_index (paper_id, content_hash, meta_hash) VALUES (?, '', '') \
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

    async fn pool() -> sqlx::SqlitePool {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let p = crate::db::connect(&url).await.unwrap();
        std::mem::forget(dir); // keep the tempdir alive for the test process
        p
    }

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

    fn two_chunks() -> Vec<Chunk> {
        vec![
            Chunk {
                seq: 0,
                page: None,
                text: "T\nA".into(),
            },
            Chunk {
                seq: 1,
                page: Some(1),
                text: "body".into(),
            },
        ]
    }

    #[test]
    fn meta_hash_changes_with_metadata_only() {
        let a = paper("p1", "h1", "Title One");
        let mut b = paper("p1", "h1", "Title One");
        assert_eq!(meta_hash(&a), meta_hash(&b));
        b.meta.title = Some("Title Two".into());
        assert_ne!(meta_hash(&a), meta_hash(&b));
    }

    #[tokio::test]
    async fn replace_chunks_roundtrip_and_stamp_lifecycle() {
        let pool = pool().await;
        let p = paper("p1", "h1", "T");
        crate::library::papers::insert(&pool, &p).await.unwrap();

        replace_chunks(&pool, "p1", &two_chunks(), "h1", &meta_hash(&p))
            .await
            .unwrap();
        let got = chunks_for_paper(&pool, "p1").await.unwrap();
        assert_eq!(got, two_chunks());
        assert_eq!(
            chunk_text(&pool, "p1", 1).await.unwrap().unwrap().text,
            "body"
        );

        let chunk_count: i64 =
            sqlx::query_scalar("SELECT chunk_count FROM search_index WHERE paper_id = 'p1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(chunk_count, 2);
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert!(row.fts_indexed_at.is_none() && row.vectors_indexed_at.is_none());

        mark_fts_done(&pool, "p1", "").await.unwrap();
        mark_vectors_done(&pool, "p1", "text-embedding-3-small")
            .await
            .unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert!(row.fts_indexed_at.is_some() && row.vectors_indexed_at.is_some());
        assert_eq!(row.embed_model.as_deref(), Some("text-embedding-3-small"));
        assert_eq!(row.fts_attempts, 0);
        assert_eq!(row.vec_attempts, 0);
        assert!(row.fts_last_error.is_none() && row.vec_last_error.is_none());

        // Replacing chunks again clears the stamps (fresh index required).
        replace_chunks(&pool, "p1", &two_chunks(), "h2", &meta_hash(&p))
            .await
            .unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert!(row.fts_indexed_at.is_none() && row.vectors_indexed_at.is_none());
        assert_eq!(row.content_hash, "h2");
    }

    #[tokio::test]
    async fn record_error_increments_only_its_tier_and_marks_reset_only_theirs() {
        let pool = pool().await;
        let p = paper("p1", "h1", "T");
        crate::library::papers::insert(&pool, &p).await.unwrap();
        replace_chunks(&pool, "p1", &two_chunks(), "h1", &meta_hash(&p))
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
        mark_fts_done(&pool, "p1", "").await.unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert_eq!(row.fts_attempts, 0);
        assert!(row.fts_last_error.is_none());
        assert_eq!(row.vec_attempts, 1);
        assert_eq!(row.vec_last_error.as_deref(), Some("embed down"));

        mark_vectors_done(&pool, "p1", "m").await.unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert_eq!(row.vec_attempts, 0);
        assert!(row.vec_last_error.is_none());
    }

    #[tokio::test]
    async fn record_error_without_prior_row_creates_one() {
        let pool = pool().await;
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
        assert_eq!(rows[0].meta_hash, "");

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
        let pool = pool().await;
        let p = paper("p1", "h1", "T");
        crate::library::papers::insert(&pool, &p).await.unwrap();
        replace_chunks(&pool, "p1", &two_chunks(), "h1", &meta_hash(&p))
            .await
            .unwrap();
        mark_fts_done(&pool, "p1", "").await.unwrap();
        mark_vectors_done(&pool, "p1", "m").await.unwrap();
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
        assert!(chunks_for_paper(&pool, "p1").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn papers_by_ids_ordered_preserves_order_and_drops_trashed() {
        let pool = pool().await;
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
        let pool = pool().await;
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
