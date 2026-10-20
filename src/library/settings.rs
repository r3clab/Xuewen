use anyhow::Result;
use sqlx::SqlitePool;

/// Read a single setting value by key.
pub(crate) async fn get(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(v,)| v))
}

/// The RFC3339 timestamp a setting was last written, if it exists.
pub(crate) async fn updated_at(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT updated_at FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(v,)| v))
}

/// Insert or overwrite a setting, stamping `updated_at` with the current time.
pub(crate) async fn set(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    let ts = crate::db::now_rfc3339();
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .bind(ts)
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove a setting (no-op if absent).
pub(crate) async fn delete(pool: &SqlitePool, key: &str) -> Result<()> {
    sqlx::query("DELETE FROM settings WHERE key = ?")
        .bind(key)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::temp_pool;

    #[tokio::test]
    async fn settings_set_get_delete_roundtrip() {
        let (_dir, pool) = temp_pool().await;
        assert_eq!(get(&pool, "proxy_cookie").await.unwrap(), None);

        set(&pool, "proxy_cookie", "ezproxy=abc").await.unwrap();
        assert_eq!(
            get(&pool, "proxy_cookie").await.unwrap().as_deref(),
            Some("ezproxy=abc")
        );

        // Upsert overwrites the value.
        set(&pool, "proxy_cookie", "ezproxy=xyz").await.unwrap();
        assert_eq!(
            get(&pool, "proxy_cookie").await.unwrap().as_deref(),
            Some("ezproxy=xyz")
        );

        // updated_at is populated.
        assert!(updated_at(&pool, "proxy_cookie").await.unwrap().is_some());

        delete(&pool, "proxy_cookie").await.unwrap();
        assert_eq!(get(&pool, "proxy_cookie").await.unwrap(), None);
        assert_eq!(updated_at(&pool, "proxy_cookie").await.unwrap(), None);
    }
}
