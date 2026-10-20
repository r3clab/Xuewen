//! Persistence for cached LLM parses of extracted reference strings.

use anyhow::Result;
use sqlx::SqlitePool;

/// Cached (parsed, provenance) for `paper_id`, only if the stored input
/// matches exactly — the comparison runs in SQL so the (up to 200kB) `refs`
/// blob never crosses the driver. Absent and stale are the same `None`; no
/// caller distinguishes them. Provenance is the `model` column:
/// `heuristic-v1` or `heuristic-v1+<model>`.
pub(crate) async fn get(
    pool: &SqlitePool,
    paper_id: &str,
    refs_json: &str,
) -> Result<Option<(String, String)>> {
    let row =
        sqlx::query_as("SELECT parsed, model FROM citation_parses WHERE paper_id = ? AND refs = ?")
            .bind(paper_id)
            .bind(refs_json)
            .fetch_optional(pool)
            .await?;
    Ok(row)
}

/// Insert or replace a paper's cached parse.
pub(crate) async fn upsert(
    pool: &SqlitePool,
    paper_id: &str,
    refs_json: &str,
    parsed_json: &str,
    model: &str,
) -> Result<()> {
    let now = crate::db::now_rfc3339();
    sqlx::query(
        "INSERT INTO citation_parses (paper_id, refs, parsed, model, created_at) \
         VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT(paper_id) DO UPDATE SET refs = excluded.refs, \
             parsed = excluded.parsed, model = excluded.model, created_at = excluded.created_at",
    )
    .bind(paper_id)
    .bind(refs_json)
    .bind(parsed_json)
    .bind(model)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::pool_with_paper;

    #[tokio::test]
    async fn upsert_then_get_roundtrips_only_on_matching_input() {
        let (pool, _dir) = pool_with_paper("p1").await;
        upsert(&pool, "p1", r#"["a","b"]"#, r#"[null,null]"#, "m")
            .await
            .unwrap();
        assert_eq!(
            get(&pool, "p1", r#"["a","b"]"#).await.unwrap(),
            Some((r#"[null,null]"#.to_string(), "m".to_string()))
        );
        // Different input (changed PDF) ⇒ miss.
        assert!(get(&pool, "p1", r#"["a","c"]"#).await.unwrap().is_none());
        // Re-upsert replaces.
        upsert(&pool, "p1", r#"["a","c"]"#, r#"[null]"#, "m")
            .await
            .unwrap();
        assert_eq!(
            get(&pool, "p1", r#"["a","c"]"#).await.unwrap(),
            Some((r#"[null]"#.to_string(), "m".to_string()))
        );
    }

    #[tokio::test]
    async fn get_on_unknown_paper_is_none() {
        let (pool, _dir) = pool_with_paper("p1").await;
        assert!(get(&pool, "nope", "[]").await.unwrap().is_none());
    }
}
