//! Persistence for per-paper summaries (`paper_summaries`).

use anyhow::Result;
use sqlx::SqlitePool;

use crate::summary::Summary;

/// The stored summary for a paper, or `None` if not yet generated. A row that
/// no longer parses (a `Summary` schema change) reads as missing; `due_ids`
/// picks the same rows up for regeneration.
pub(crate) async fn get(pool: &SqlitePool, paper_id: &str) -> Result<Option<Summary>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT summary FROM paper_summaries WHERE paper_id = ?")
            .bind(paper_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(json,)| match serde_json::from_str(&json) {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::warn!(
                "stored summary for {paper_id} no longer parses ({e}); it will be regenerated"
            );
            None
        }
    }))
}

/// Whether the paper joined as `p` still needs a summary: none stored as `s`,
/// or one `get` cannot read. Mirrors `Summary`'s required string fields; CASE
/// rather than AND because `json_type` raises on malformed JSON and only CASE
/// guarantees `json_valid` runs first.
macro_rules! wants_summary {
    () => {
        "(s.paper_id IS NULL OR NOT CASE WHEN json_valid(s.summary) THEN \
           json_type(s.summary, '$.tldr') IS 'text' \
           AND json_type(s.summary, '$.problem') IS 'text' \
           AND json_type(s.summary, '$.approach') IS 'text' \
           AND json_type(s.summary, '$.results') IS 'text' \
           AND json_type(s.summary, '$.limitations') IS 'text' \
         ELSE 0 END)"
    };
}

/// Insert or replace a paper's summary.
pub async fn upsert(pool: &SqlitePool, paper_id: &str, s: &Summary, model: &str) -> Result<()> {
    let json = serde_json::to_string(s)?;
    let now = crate::db::now_rfc3339();
    sqlx::query(
        "INSERT INTO paper_summaries (paper_id, summary, model, generated_at) \
         VALUES (?, ?, ?, ?) \
         ON CONFLICT(paper_id) DO UPDATE SET summary = excluded.summary, \
             model = excluded.model, generated_at = excluded.generated_at",
    )
    .bind(paper_id)
    .bind(&json)
    .bind(model)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Ids of live papers due for a summary: none readable stored, and either
/// never failed or past the backoff window (`last_attempt_at < retry_before`)
/// and under `max_attempts`. Never-failed papers sort first so failures never
/// starve the queue.
pub(crate) async fn due_ids(
    pool: &SqlitePool,
    limit: i64,
    max_attempts: i64,
    retry_before: &str,
) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(concat!(
        "SELECT p.id FROM papers p \
         LEFT JOIN paper_summaries s ON s.paper_id = p.id \
         LEFT JOIN summary_failures f ON f.paper_id = p.id \
         WHERE p.deleted_at IS NULL AND ",
        wants_summary!(),
        " AND (f.paper_id IS NULL OR (f.attempts < ? AND f.last_attempt_at < ?)) \
         ORDER BY (f.paper_id IS NOT NULL), p.added_at DESC \
         LIMIT ?"
    ))
    .bind(max_attempts)
    .bind(retry_before)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

pub(crate) async fn earliest_retryable_failure(
    pool: &SqlitePool,
    max_attempts: i64,
) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(concat!(
        "SELECT MIN(f.last_attempt_at) FROM summary_failures f \
         JOIN papers p ON p.id = f.paper_id \
         LEFT JOIN paper_summaries s ON s.paper_id = p.id \
         WHERE p.deleted_at IS NULL AND ",
        wants_summary!(),
        " AND f.attempts < ?"
    ))
    .bind(max_attempts)
    .fetch_one(pool)
    .await?)
}

/// Record (or increment) a failed generation attempt.
pub(crate) async fn record_failure(pool: &SqlitePool, paper_id: &str) -> Result<()> {
    let now = crate::db::now_rfc3339();
    sqlx::query(
        "INSERT INTO summary_failures (paper_id, attempts, last_attempt_at) \
         VALUES (?, 1, ?) \
         ON CONFLICT(paper_id) DO UPDATE SET attempts = attempts + 1, \
             last_attempt_at = excluded.last_attempt_at",
    )
    .bind(paper_id)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Clear one paper's failure record (`Some`) or all of them (`None`).
pub(crate) async fn clear_failure(pool: &SqlitePool, paper_id: Option<&str>) -> Result<()> {
    match paper_id {
        Some(id) => {
            sqlx::query("DELETE FROM summary_failures WHERE paper_id = ?")
                .bind(id)
                .execute(pool)
                .await?;
        }
        None => {
            sqlx::query("DELETE FROM summary_failures")
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::model::{Authors, Paper, PaperMeta, PaperStatus};

    fn paper(id: &str) -> Paper {
        Paper {
            id: id.into(),
            content_hash: format!("h-{id}"),
            rel_path: format!("{id}.pdf"),
            cite_key: None,
            added_at: "2026-07-12T00:00:00Z".into(),
            deleted_at: None,
            starred: false,
            name: None,
            meta: PaperMeta {
                title: Some("T".into()),
                abstract_text: Some("A".into()),
                authors: Authors(vec!["Ada".into()]),
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

    fn sample() -> Summary {
        Summary {
            tldr: "One line.".into(),
            problem: "Gap.".into(),
            approach: "Idea.".into(),
            results: "+4.2.".into(),
            limitations: "Small.".into(),
        }
    }

    // High cap, far-future cutoff: no failure rows exist in most tests, so this
    // returns every un-summarized live paper (mirrors old `missing_ids` behavior).
    const FUTURE_CUTOFF: &str = "9999-01-01T00:00:00Z";

    #[tokio::test]
    async fn upsert_get_and_missing() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("p1"))
            .await
            .unwrap();
        crate::library::papers::insert(&pool, &paper("p2"))
            .await
            .unwrap();

        // Both papers start missing.
        let mut missing = due_ids(&pool, 10, 100, FUTURE_CUTOFF).await.unwrap();
        missing.sort();
        assert_eq!(missing, vec!["p1".to_string(), "p2".to_string()]);

        upsert(&pool, "p1", &sample(), "gpt-x").await.unwrap();
        assert_eq!(get(&pool, "p1").await.unwrap(), Some(sample()));
        assert_eq!(get(&pool, "p2").await.unwrap(), None);
        assert_eq!(
            due_ids(&pool, 10, 100, FUTURE_CUTOFF).await.unwrap(),
            vec!["p2".to_string()]
        );

        // upsert replaces.
        let mut two = sample();
        two.tldr = "Changed.".into();
        upsert(&pool, "p1", &two, "gpt-x").await.unwrap();
        assert_eq!(get(&pool, "p1").await.unwrap().unwrap().tldr, "Changed.");
    }

    #[tokio::test]
    async fn an_unparseable_row_reads_as_missing_and_requeues_the_paper() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("p1"))
            .await
            .unwrap();
        // A stored summary an older/newer schema wrote, no longer parseable.
        sqlx::query(
            "INSERT INTO paper_summaries (paper_id, summary, model, generated_at) \
             VALUES ('p1', '{\"tldr\": only}', 'gpt-x', '2026-01-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert_eq!(get(&pool, "p1").await.unwrap(), None);
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM paper_summaries")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 1, "a read never deletes");
        assert_eq!(
            due_ids(&pool, 10, 100, FUTURE_CUTOFF).await.unwrap(),
            vec!["p1".to_string()]
        );

        upsert(&pool, "p1", &sample(), "gpt-x").await.unwrap();
        assert!(due_ids(&pool, 10, 100, FUTURE_CUTOFF)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn due_ids_requeues_exactly_the_rows_get_cannot_read() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("p1"))
            .await
            .unwrap();
        let full = serde_json::to_value(sample()).unwrap();
        let mut variants = vec![full.clone()];
        for key in full.as_object().unwrap().keys() {
            let mut missing = full.clone();
            missing.as_object_mut().unwrap().remove(key);
            variants.push(missing);
            let mut mistyped = full.clone();
            mistyped[key] = serde_json::json!(1);
            variants.push(mistyped);
        }
        for v in variants {
            sqlx::query("INSERT OR REPLACE INTO paper_summaries (paper_id, summary, model, generated_at) VALUES ('p1', ?, 'm', '2026-01-01T00:00:00Z')")
                .bind(v.to_string())
                .execute(&pool)
                .await
                .unwrap();
            let unreadable = get(&pool, "p1").await.unwrap().is_none();
            let due = !due_ids(&pool, 10, 100, FUTURE_CUTOFF)
                .await
                .unwrap()
                .is_empty();
            assert_eq!(due, unreadable, "{v}");
        }
    }

    #[tokio::test]
    async fn the_earliest_retryable_failure_ignores_exhausted_and_summarized_papers() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        for id in ["p1", "p2", "p3"] {
            crate::library::papers::insert(&pool, &paper(id))
                .await
                .unwrap();
        }
        assert_eq!(earliest_retryable_failure(&pool, 3).await.unwrap(), None);
        for (id, attempts, at) in [
            ("p1", 3, "2026-01-01T00:00:00+00:00"),
            ("p2", 1, "2026-01-03T00:00:00+00:00"),
            ("p3", 1, "2026-01-02T00:00:00+00:00"),
        ] {
            sqlx::query(
                "INSERT INTO summary_failures (paper_id, attempts, last_attempt_at) VALUES (?, ?, ?)",
            )
            .bind(id)
            .bind(attempts)
            .bind(at)
            .execute(&pool)
            .await
            .unwrap();
        }
        upsert(&pool, "p3", &sample(), "m").await.unwrap();
        assert_eq!(
            earliest_retryable_failure(&pool, 3)
                .await
                .unwrap()
                .as_deref(),
            Some("2026-01-03T00:00:00+00:00")
        );
    }

    #[test]
    fn legacy_five_field_json_still_deserializes() {
        // Rows written by the original five-field schema must keep parsing;
        // new `Summary` fields need `#[serde(default)]` (or every stored
        // summary reads as missing until the sweep regenerates it).
        let legacy =
            r#"{"tldr":"T.","problem":"P.","approach":"A.","results":"R.","limitations":"L."}"#;
        let s: Summary = serde_json::from_str(legacy).unwrap();
        assert_eq!(s.tldr, "T.");
        assert_eq!(s.limitations, "L.");
    }

    #[tokio::test]
    async fn due_ids_excludes_trashed() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("p1"))
            .await
            .unwrap();
        crate::library::papers::insert(&pool, &paper("p2"))
            .await
            .unwrap();

        crate::library::trash::soft_delete(&pool, "p2")
            .await
            .unwrap();

        let missing = due_ids(&pool, 10, 100, FUTURE_CUTOFF).await.unwrap();
        assert_eq!(missing, vec!["p1".to_string()]);
    }

    #[tokio::test]
    async fn due_ids_skips_recently_failed() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("p1"))
            .await
            .unwrap();
        record_failure(&pool, "p1").await.unwrap();

        // last_attempt_at is "now", which is NOT before a cutoff far in the past.
        let past_cutoff = "2000-01-01T00:00:00Z";
        assert_eq!(
            due_ids(&pool, 10, 5, past_cutoff).await.unwrap(),
            Vec::<String>::new()
        );

        // Eligible again once the cutoff is far in the future.
        assert_eq!(
            due_ids(&pool, 10, 5, FUTURE_CUTOFF).await.unwrap(),
            vec!["p1".to_string()]
        );
    }

    #[tokio::test]
    async fn due_ids_skips_at_max_attempts() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("p1"))
            .await
            .unwrap();
        record_failure(&pool, "p1").await.unwrap();
        record_failure(&pool, "p1").await.unwrap();
        record_failure(&pool, "p1").await.unwrap();

        // attempts == 3, not < 3 -> excluded.
        assert_eq!(
            due_ids(&pool, 10, 3, FUTURE_CUTOFF).await.unwrap(),
            Vec::<String>::new()
        );
        // attempts == 3 < 4 -> included.
        assert_eq!(
            due_ids(&pool, 10, 4, FUTURE_CUTOFF).await.unwrap(),
            vec!["p1".to_string()]
        );
    }

    #[tokio::test]
    async fn due_ids_untried_before_failed() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        let mut p_old = paper("p_old");
        p_old.added_at = "2026-01-01T00:00:00Z".into();
        let mut p_new = paper("p_new");
        p_new.added_at = "2026-06-01T00:00:00Z".into();
        crate::library::papers::insert(&pool, &p_old).await.unwrap();
        crate::library::papers::insert(&pool, &p_new).await.unwrap();

        record_failure(&pool, "p_new").await.unwrap();

        let ids = due_ids(&pool, 10, 5, FUTURE_CUTOFF).await.unwrap();
        assert_eq!(ids, vec!["p_old".to_string(), "p_new".to_string()]);
    }

    #[tokio::test]
    async fn clear_failure_removes_row() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("p1"))
            .await
            .unwrap();
        record_failure(&pool, "p1").await.unwrap();
        assert_eq!(
            due_ids(&pool, 10, 5, "2000-01-01T00:00:00Z").await.unwrap(),
            Vec::<String>::new()
        );

        clear_failure(&pool, Some("p1")).await.unwrap();
        assert_eq!(
            due_ids(&pool, 10, 5, "2000-01-01T00:00:00Z").await.unwrap(),
            vec!["p1".to_string()]
        );
    }

    #[tokio::test]
    async fn cascade_delete_removes_summary() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("p1"))
            .await
            .unwrap();
        upsert(&pool, "p1", &sample(), "gpt-x").await.unwrap();
        assert_eq!(get(&pool, "p1").await.unwrap(), Some(sample()));

        crate::library::trash::hard_delete(&pool, "p1")
            .await
            .unwrap();
        assert_eq!(get(&pool, "p1").await.unwrap(), None);
    }
}
