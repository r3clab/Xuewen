//! SQLite persistence for chat threads. Writes are all-or-nothing per
//! exchange: nothing is stored for aborted or failed generations, so the
//! thread only ever contains completed exchanges.

use anyhow::Result;
use sqlx::SqlitePool;

use super::ask::ToolCall;

// Storage row, not a wire type: the history endpoint maps it to
// `web::handlers::chat::ChatTurn`, and `xuewen chat history` prints it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ChatMessageRow {
    pub(crate) id: i64,
    pub role: String,
    pub content: String,
    pub model: Option<String>,
    pub(crate) created_at: String,
    pub tools_json: Option<String>,
}

impl ChatMessageRow {
    /// The turn's tool log. A row whose `tools_json` no longer parses
    /// degrades to no tools rather than failing the whole thread.
    pub(crate) fn tools(&self) -> Option<Vec<ToolCall>> {
        self.tools_json.as_deref().and_then(|s| {
            serde_json::from_str(s)
                .map_err(|e| tracing::warn!("chat turn {}: unparseable tools_json: {e}", self.id))
                .ok()
        })
    }
}

pub async fn list(pool: &SqlitePool, paper_id: &str) -> Result<Vec<ChatMessageRow>> {
    Ok(sqlx::query_as::<_, ChatMessageRow>(
        "SELECT id, role, content, model, created_at, tools_json
         FROM chat_messages WHERE paper_id = ? ORDER BY id",
    )
    .bind(paper_id)
    .fetch_all(pool)
    .await?)
}

/// Persist one completed exchange atomically; returns the assistant row id.
pub async fn insert_exchange(
    pool: &SqlitePool,
    paper_id: &str,
    user_content: &str,
    assistant_content: &str,
    model_label: &str,
    tools_json: Option<&str>,
) -> Result<i64> {
    // One explicit RFC3339 stamp shared by both rows (ordering stays by id) —
    // the column's `datetime('now')` default is the wrong format, and it was
    // this table's only remaining writer of it.
    let created_at = crate::db::now_rfc3339();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO chat_messages (paper_id, role, content, created_at) VALUES (?, 'user', ?, ?)",
    )
    .bind(paper_id)
    .bind(user_content)
    .bind(&created_at)
    .execute(&mut *tx)
    .await?;
    let res = sqlx::query(
        "INSERT INTO chat_messages (paper_id, role, content, model, tools_json, created_at) VALUES (?, 'assistant', ?, ?, ?, ?)",
    )
    .bind(paper_id)
    .bind(assistant_content)
    .bind(model_label)
    .bind(tools_json)
    .bind(&created_at)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(res.last_insert_rowid())
}

pub(crate) async fn count(pool: &SqlitePool, paper_id: &str) -> Result<i64> {
    Ok(
        sqlx::query_scalar("SELECT COUNT(*) FROM chat_messages WHERE paper_id = ?")
            .bind(paper_id)
            .fetch_one(pool)
            .await?,
    )
}

pub(crate) async fn clear(pool: &SqlitePool, paper_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM chat_messages WHERE paper_id = ?")
        .bind(paper_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::pool_with_paper;

    #[tokio::test]
    async fn exchange_roundtrip_in_order() {
        let (pool, _dir) = pool_with_paper("p1").await;
        let aid = insert_exchange(
            &pool,
            "p1",
            "what is this?",
            "a paper.",
            "GPT-5 Mini",
            Some(r#"[{"name":"Read","detail":"paper.txt"}]"#),
        )
        .await
        .unwrap();
        assert!(aid > 0);
        insert_exchange(
            &pool,
            "p1",
            "and the method?",
            "transformers.",
            "Local Qwen",
            None,
        )
        .await
        .unwrap();

        let rows = list(&pool, "p1").await.unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].role, "user");
        assert_eq!(rows[0].content, "what is this?");
        assert_eq!(rows[0].model, None);
        assert_eq!(rows[1].role, "assistant");
        assert_eq!(rows[1].model.as_deref(), Some("GPT-5 Mini"));
        assert_eq!(
            rows[1].tools_json.as_deref(),
            Some(r#"[{"name":"Read","detail":"paper.txt"}]"#)
        );
        assert_eq!(rows[3].model.as_deref(), Some("Local Qwen"));
        assert_eq!(rows[3].tools_json, None);
        // Both rows of an exchange share one stamp, in RFC3339 like every
        // other stored timestamp (not the column default's SQLite format).
        assert_eq!(rows[0].created_at, rows[1].created_at);
        assert!(
            chrono::DateTime::parse_from_rfc3339(&rows[0].created_at).is_ok(),
            "created_at not RFC3339: {}",
            rows[0].created_at
        );
    }

    #[tokio::test]
    async fn clear_empties_one_thread_only() {
        let (pool, _dir) = pool_with_paper("p1").await;
        sqlx::query(
            "INSERT INTO papers (id, content_hash, rel_path, added_at, status)
             VALUES ('p2', 'hash2', 'q.pdf', datetime('now'), 'resolved')",
        )
        .execute(&pool)
        .await
        .unwrap();
        insert_exchange(&pool, "p1", "q", "a", "M", None)
            .await
            .unwrap();
        insert_exchange(&pool, "p2", "q", "a", "M", None)
            .await
            .unwrap();

        clear(&pool, "p1").await.unwrap();
        assert!(list(&pool, "p1").await.unwrap().is_empty());
        assert_eq!(list(&pool, "p2").await.unwrap().len(), 2);
    }
}
