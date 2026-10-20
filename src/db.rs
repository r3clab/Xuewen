use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

/// Open (creating if needed) the SQLite database and run migrations.
pub async fn connect(database_url: &str) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(10));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// The single mint point for stored timestamps. Everything this app writes is
/// RFC3339 (`…T…+00:00`) — never SQL `datetime('now')`, whose format neither
/// compares lexicographically against RFC3339 nor parses as UTC in JS `Date`.
pub(crate) fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Whether `e` (from a db call) is a UNIQUE-constraint violation.
pub(crate) fn is_unique_violation(e: &anyhow::Error) -> bool {
    e.downcast_ref::<sqlx::Error>()
        .and_then(|e| e.as_database_error())
        .is_some_and(|d| d.kind() == sqlx::error::ErrorKind::UniqueViolation)
}

#[cfg(test)]
mod tests {
    use crate::testutil::temp_pool;

    #[tokio::test]
    async fn connect_uses_wal_journal_mode() {
        let (_dir, pool) = temp_pool().await;
        let (mode,): (String,) = sqlx::query_as("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }
}
