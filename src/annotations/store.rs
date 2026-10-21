//! Persistence for reader annotations.

use std::collections::HashMap;

use anyhow::Result;
use sqlx::SqlitePool;

use super::{Annotation, NewAnnotation};

const COLUMNS: &str = "paper_id, id, page_index, kind, color, quoted_text, note, payload, \
                       created_at, updated_at";
/// Reading order: down the document, then oldest-first within a page. `id`
/// only breaks ties so the order is total and stable across queries.
const ORDER: &str = "ORDER BY page_index, created_at, id";

/// The row as stored. `payload` is TEXT here and decoded in `into_domain` —
/// the same shape as `citation_parses.parsed` and `paper_summaries.summary`,
/// rather than a custom `sqlx::Decode` impl.
#[derive(Debug, Clone, sqlx::FromRow)]
struct AnnotationRow {
    paper_id: String,
    id: String,
    page_index: i64,
    kind: super::AnnotationKind,
    color: super::AnnotationColor,
    quoted_text: Option<String>,
    note: Option<String>,
    payload: String,
    created_at: String,
    updated_at: String,
}

impl AnnotationRow {
    /// Unparseable payload degrades to `Null` rather than failing the whole
    /// list: the row's projection (page, kind, note) is still good enough to
    /// show in the sidebar and the CLI, and only that one mark goes
    /// un-redrawn. Warned once per row because annotations are fetched once
    /// per document open, not polled — this cannot spam the log.
    fn into_domain(self) -> Annotation {
        let payload = serde_json::from_str(&self.payload).unwrap_or_else(|e| {
            tracing::warn!(
                "annotation {}/{}: unreadable payload ({e}); it will not be redrawn",
                self.paper_id,
                self.id
            );
            serde_json::Value::Null
        });
        Annotation {
            paper_id: self.paper_id,
            id: self.id,
            page_index: self.page_index,
            kind: self.kind,
            color: self.color,
            quoted_text: self.quoted_text,
            note: self.note,
            payload,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// A paper's annotations in reading order.
pub(crate) async fn list_for_paper(pool: &SqlitePool, paper_id: &str) -> Result<Vec<Annotation>> {
    let rows = sqlx::query_as::<_, AnnotationRow>(&format!(
        "SELECT {COLUMNS} FROM annotations WHERE paper_id = ? {ORDER}"
    ))
    .bind(paper_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(AnnotationRow::into_domain).collect())
}

/// Insert, or overwrite a mark that already carries this id. `created_at` is
/// deliberately left alone on conflict so a retried save doesn't reset it.
/// One statement, `RETURNING` the row as stored (after `ON CONFLICT`, so the
/// preserved `created_at` comes back) — a separate read-back would let a
/// concurrent delete turn a successful write into a phantom miss.
pub(crate) async fn upsert(
    pool: &SqlitePool,
    paper_id: &str,
    id: &str,
    new: &NewAnnotation,
    now: &str,
) -> Result<Annotation> {
    let row = sqlx::query_as::<_, AnnotationRow>(&format!(
        "INSERT INTO annotations \
         (paper_id, id, page_index, kind, color, quoted_text, note, payload, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(paper_id, id) DO UPDATE SET \
             page_index = excluded.page_index, kind = excluded.kind, color = excluded.color, \
             quoted_text = excluded.quoted_text, note = excluded.note, \
             payload = excluded.payload, updated_at = excluded.updated_at \
         RETURNING {COLUMNS}"
    ))
    .bind(paper_id)
    .bind(id)
    .bind(new.page_index)
    .bind(new.kind)
    .bind(new.color)
    .bind(blank_to_none(new.quoted_text.as_deref()))
    .bind(blank_to_none(new.note.as_deref()))
    .bind(serde_json::to_string(&new.payload)?)
    .bind(now)
    .bind(now)
    .fetch_one(pool)
    .await?;
    Ok(row.into_domain())
}

/// Empty strings normalize to NULL at the bind so "cleared" and "never set"
/// are the same in storage — for every caller, not just the service — and
/// neither reaches the search index as blank text.
fn blank_to_none(s: Option<&str>) -> Option<&str> {
    s.filter(|v| !v.trim().is_empty())
}

/// Whether a row was actually removed.
pub(crate) async fn delete(pool: &SqlitePool, paper_id: &str, id: &str) -> Result<bool> {
    let res = sqlx::query("DELETE FROM annotations WHERE paper_id = ? AND id = ?")
        .bind(paper_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub(crate) async fn count_for_paper(pool: &SqlitePool, paper_id: &str) -> Result<i64> {
    Ok(
        sqlx::query_scalar("SELECT COUNT(*) FROM annotations WHERE paper_id = ?")
            .bind(paper_id)
            .fetch_one(pool)
            .await?,
    )
}

/// Drop every annotation on a paper; returns how many rows went.
pub(crate) async fn delete_all_for_paper(pool: &SqlitePool, paper_id: &str) -> Result<u64> {
    let res = sqlx::query("DELETE FROM annotations WHERE paper_id = ?")
        .bind(paper_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

/// The text the `notes` search field indexes for one paper: every non-empty
/// note, in reading order, newline-joined. Empty when the paper has none.
pub(crate) async fn notes_blob(pool: &SqlitePool, paper_id: &str) -> Result<String> {
    let notes: Vec<String> = sqlx::query_scalar(&format!(
        "SELECT note FROM annotations WHERE paper_id = ? AND note IS NOT NULL AND note <> '' {ORDER}"
    ))
    .bind(paper_id)
    .fetch_all(pool)
    .await?;
    Ok(join_notes(notes))
}

/// Every paper's notes blob in one query — the search planner needs the whole
/// library at once and must not issue a query per paper. Papers with no notes
/// are absent from the map (the caller treats a miss as the empty blob).
pub(crate) async fn notes_by_paper(pool: &SqlitePool) -> Result<HashMap<String, String>> {
    // Ordered by paper first so each group arrives contiguously, then by the
    // same reading order `ORDER` spells out for the single-paper query.
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT paper_id, note FROM annotations \
         WHERE note IS NOT NULL AND note <> '' ORDER BY paper_id, page_index, created_at, id",
    )
    .fetch_all(pool)
    .await?;
    let mut grouped: HashMap<String, Vec<String>> = HashMap::new();
    for (paper_id, note) in rows {
        grouped.entry(paper_id).or_default().push(note);
    }
    Ok(grouped
        .into_iter()
        .map(|(id, notes)| (id, join_notes(notes)))
        .collect())
}

/// The single join rule, shared so the hashed blob and the indexed text can
/// never disagree about what a paper's notes are.
fn join_notes(notes: Vec<String>) -> String {
    notes.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotations::{AnnotationColor, AnnotationKind};
    use crate::testutil::pool_with_paper;

    fn payload(tag: &str) -> serde_json::Value {
        serde_json::json!({ "annotation": { "type": 9, "tag": tag } })
    }

    const T0: &str = "2026-08-14T00:00:00Z";
    const T1: &str = "2026-08-14T01:00:00Z";

    /// Write one highlight onto the seeded paper. The id also seeds the
    /// payload, so a round trip can prove the right blob came back.
    async fn put(
        pool: &SqlitePool,
        id: &str,
        page_index: i64,
        note: Option<&str>,
        now: &str,
    ) -> Annotation {
        let new = NewAnnotation {
            page_index,
            kind: AnnotationKind::Highlight,
            color: AnnotationColor::Amber,
            quoted_text: Some("marked text".into()),
            note: note.map(str::to_string),
            payload: payload(id),
        };
        upsert(pool, "p1", id, &new, now).await.unwrap()
    }

    #[tokio::test]
    async fn upsert_then_list_roundtrips_the_payload() {
        let (pool, _dir) = pool_with_paper("p1").await;
        let a = put(&pool, "a1", 3, Some("a note"), T0).await;
        assert_eq!(a.page_index, 3);
        assert_eq!(a.kind, AnnotationKind::Highlight);
        assert_eq!(a.note.as_deref(), Some("a note"));
        assert_eq!(a.payload, payload("a1"));

        let all = list_for_paper(&pool, "p1").await.unwrap();
        assert_eq!(all, vec![a]);
    }

    #[tokio::test]
    async fn list_is_in_page_then_creation_order() {
        let (pool, _dir) = pool_with_paper("p1").await;
        put(&pool, "late", 1, None, T1).await;
        put(&pool, "early", 1, None, T0).await;
        put(&pool, "page0", 0, None, T1).await;
        let ids: Vec<_> = list_for_paper(&pool, "p1")
            .await
            .unwrap()
            .into_iter()
            .map(|a| a.id)
            .collect();
        assert_eq!(ids, ["page0", "early", "late"]);
    }

    #[tokio::test]
    async fn upsert_is_idempotent_and_keeps_created_at() {
        let (pool, _dir) = pool_with_paper("p1").await;
        put(&pool, "a1", 0, None, T0).await;
        let again = put(&pool, "a1", 7, Some("added"), T1).await;
        assert_eq!(list_for_paper(&pool, "p1").await.unwrap().len(), 1);
        assert_eq!(again.page_index, 7);
        assert_eq!(again.created_at, T0, "a retried save must not reset this");
        assert_eq!(again.updated_at, T1);
    }

    #[tokio::test]
    async fn a_blank_note_or_quote_is_stored_as_null() {
        // Normalization lives in `upsert` itself, so it holds for every
        // caller — not only writes routed through the service.
        let (pool, _dir) = pool_with_paper("p1").await;
        let new = NewAnnotation {
            page_index: 0,
            kind: AnnotationKind::Highlight,
            color: AnnotationColor::Amber,
            quoted_text: Some(String::new()),
            note: Some("   ".into()),
            payload: payload("a1"),
        };
        let a = upsert(&pool, "p1", "a1", &new, T0).await.unwrap();
        assert_eq!(a.quoted_text, None);
        assert_eq!(a.note, None);
    }

    #[test]
    fn blank_text_normalizes_to_none() {
        assert_eq!(blank_to_none(Some("  ")), None);
        assert_eq!(blank_to_none(Some("hi")), Some("hi"));
        assert_eq!(blank_to_none(None), None);
    }

    #[tokio::test]
    async fn delete_reports_whether_a_row_went() {
        let (pool, _dir) = pool_with_paper("p1").await;
        put(&pool, "a1", 0, None, T0).await;
        assert!(delete(&pool, "p1", "a1").await.unwrap());
        assert!(!delete(&pool, "p1", "a1").await.unwrap());
        assert!(list_for_paper(&pool, "p1").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_all_returns_the_count() {
        let (pool, _dir) = pool_with_paper("p1").await;
        put(&pool, "a1", 0, None, T0).await;
        put(&pool, "a2", 1, None, T0).await;
        assert_eq!(delete_all_for_paper(&pool, "p1").await.unwrap(), 2);
        assert_eq!(delete_all_for_paper(&pool, "p1").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn annotations_go_when_their_paper_is_purged() {
        let (pool, _dir) = pool_with_paper("p1").await;
        put(&pool, "a1", 0, None, T0).await;
        crate::library::trash::hard_delete(&pool, "p1")
            .await
            .unwrap();
        assert!(list_for_paper(&pool, "p1").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn notes_blob_joins_only_non_empty_notes_in_order() {
        let (pool, _dir) = pool_with_paper("p1").await;
        put(&pool, "a2", 2, Some("second"), T0).await;
        put(&pool, "a1", 1, Some("first"), T0).await;
        put(&pool, "a0", 0, None, T0).await;
        assert_eq!(notes_blob(&pool, "p1").await.unwrap(), "first\nsecond");
    }

    #[tokio::test]
    async fn notes_blob_is_empty_without_notes() {
        let (pool, _dir) = pool_with_paper("p1").await;
        put(&pool, "a1", 0, None, T0).await;
        assert_eq!(notes_blob(&pool, "p1").await.unwrap(), "");
    }

    #[tokio::test]
    async fn notes_by_paper_agrees_with_the_per_paper_blob() {
        let (pool, _dir) = pool_with_paper("p1").await;
        put(&pool, "a1", 0, Some("alpha"), T0).await;
        put(&pool, "a2", 1, Some("beta"), T0).await;
        let map = notes_by_paper(&pool).await.unwrap();
        assert_eq!(
            map.get("p1").map(String::as_str),
            Some(notes_blob(&pool, "p1").await.unwrap().as_str())
        );
        // Papers with no notes are absent rather than mapped to "".
        assert!(!map.contains_key("absent"));
    }

    #[tokio::test]
    async fn unreadable_payload_still_lists_with_its_projection() {
        let (pool, _dir) = pool_with_paper("p1").await;
        put(&pool, "a1", 4, Some("kept"), T0).await;
        sqlx::query("UPDATE annotations SET payload = 'not json' WHERE id = 'a1'")
            .execute(&pool)
            .await
            .unwrap();
        let all = list_for_paper(&pool, "p1").await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].note.as_deref(), Some("kept"));
        assert_eq!(all[0].page_index, 4);
        assert!(all[0].payload.is_null(), "unredrawable, but not lost");
    }
}
