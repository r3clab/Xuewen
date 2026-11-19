use anyhow::Result;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{ConnectOptions, SqliteConnection, SqlitePool};
use std::str::FromStr;
use std::time::Duration;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Open (creating if needed) the SQLite database and run migrations.
pub async fn connect(database_url: &str) -> Result<SqlitePool> {
    let pool = open(database_url).await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

async fn open(database_url: &str) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(10));
    Ok(SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?)
}

/// The single mint point for stored timestamps. Everything this app writes is
/// RFC3339 (`…T…+00:00`) — never SQL `datetime('now')`, whose format neither
/// compares lexicographically against RFC3339 nor parses as UTC in JS `Date`.
pub(crate) fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Notices commits made through any other connection — this process's pool
/// or another process altogether, such as a CLI command run beside the
/// server — by polling SQLite's `data_version` on a connection of its own.
pub(crate) struct ChangeWatch {
    conn: SqliteConnection,
    seen: i64,
}

impl ChangeWatch {
    const POLL: Duration = Duration::from_secs(2);

    pub(crate) async fn open(pool: &SqlitePool) -> Result<Self> {
        let mut conn = pool.connect_options().connect().await?;
        let seen = data_version(&mut conn).await?;
        Ok(Self { conn, seen })
    }

    /// Count every commit so far as seen.
    pub(crate) async fn mark_seen(&mut self) -> Result<()> {
        self.seen = data_version(&mut self.conn).await?;
        Ok(())
    }

    /// Resolves once a commit lands after the last `mark_seen`; never, once
    /// the database cannot be read.
    pub(crate) async fn changed(&mut self) {
        loop {
            tokio::time::sleep(Self::POLL).await;
            match data_version(&mut self.conn).await {
                Ok(v) if v != self.seen => return,
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("no longer watching the database for changes: {e:#}");
                    return std::future::pending().await;
                }
            }
        }
    }
}

async fn data_version(conn: &mut SqliteConnection) -> Result<i64> {
    Ok(sqlx::query_scalar("PRAGMA data_version")
        .fetch_one(conn)
        .await?)
}

/// Whether `e` (from a db call) is a UNIQUE-constraint violation.
pub(crate) fn is_unique_violation(e: &anyhow::Error) -> bool {
    e.downcast_ref::<sqlx::Error>().is_some_and(violates_unique)
}

pub(crate) fn violates_unique(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .is_some_and(|d| d.kind() == sqlx::error::ErrorKind::UniqueViolation)
}

/// Escape `\`, `%`, `_` in user input for `LIKE … ESCAPE '\'`.
pub(crate) fn escape_like(term: &str) -> String {
    term.replace('\\', r"\\")
        .replace('%', r"\%")
        .replace('_', r"\_")
}

/// A database migrated up to, but not including, the migration named
/// `description`, for testing what that migration does to existing rows.
#[cfg(test)]
pub(crate) async fn pool_before_migration(description: &str) -> (tempfile::TempDir, SqlitePool) {
    let version = MIGRATOR
        .iter()
        .find(|m| m.description == description)
        .unwrap_or_else(|| panic!("no migration named {description:?}"))
        .version;
    let dir = tempfile::tempdir().unwrap();
    let pool = open(&format!("sqlite:{}", dir.path().join("t.db").display()))
        .await
        .unwrap();
    MIGRATOR.run_to(version - 1, &pool).await.unwrap();
    (dir, pool)
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn escape_like_escapes_backslash_percent_and_underscore() {
        assert_eq!(escape_like("100%"), r"100\%");
        assert_eq!(escape_like("a_b"), r"a\_b");
        assert_eq!(escape_like(r"back\slash"), r"back\\slash");
        assert_eq!(escape_like("%_\\"), r"\%\_\\");
    }

    async fn seed(pool: &SqlitePool, id: &str, added_at: &str, cols: &[(&str, &str)]) {
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "INSERT INTO papers (id, content_hash, rel_path, status, added_at",
        );
        for (c, _) in cols {
            qb.push(", ").push(c);
        }
        qb.push(") VALUES (");
        let mut values = qb.separated(", ");
        values
            .push_bind(id)
            .push_bind(id)
            .push_bind(format!("{id}.pdf"))
            .push_bind("resolved")
            .push_bind(added_at);
        for (_, v) in cols {
            values.push_bind(*v);
        }
        qb.push(")");
        qb.build().execute(pool).await.unwrap();
    }

    async fn row(pool: &SqlitePool, id: &str) -> (Option<String>, Option<String>, String) {
        sqlx::query_as("SELECT doi, arxiv_id, status FROM papers WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn canonical_identifiers_migration_folds_and_settles_collisions() {
        let (_dir, pool) = pool_before_migration("canonical identifiers").await;
        seed(
            &pool,
            "a",
            "2026-01-01T00:00:00+00:00",
            &[("doi", "10.1145/AbC")],
        )
        .await;
        seed(
            &pool,
            "b",
            "2026-01-02T00:00:00+00:00",
            &[("doi", "10.1145/abc")],
        )
        .await;
        seed(
            &pool,
            "c",
            "2026-01-03T00:00:00+00:00",
            &[("arxiv_id", "1706.03762v5")],
        )
        .await;
        seed(
            &pool,
            "d",
            "2026-01-04T00:00:00+00:00",
            &[("arxiv_id", "1706.03762")],
        )
        .await;
        seed(
            &pool,
            "e",
            "2026-01-05T00:00:00+00:00",
            &[("arxiv_id", "math.NA/0303029v1")],
        )
        .await;
        seed(
            &pool,
            "f",
            "2026-01-06T00:00:00+00:00",
            &[("arxiv_id", "cs.CV/9901002")],
        )
        .await;
        MIGRATOR.run(&pool).await.unwrap();

        // The oldest row keeps the identifier, canonicalised…
        assert_eq!(
            row(&pool, "a").await,
            (Some("10.1145/abc".into()), None, "resolved".into())
        );
        assert_eq!(
            row(&pool, "c").await,
            (None, Some("1706.03762".into()), "resolved".into())
        );
        // …later collisions lose it and go back to review.
        assert_eq!(row(&pool, "b").await, (None, None, "needs_review".into()));
        assert_eq!(row(&pool, "d").await, (None, None, "needs_review".into()));
        // Old-style ids: only a trailing version is stripped.
        assert_eq!(row(&pool, "e").await.1.as_deref(), Some("math.NA/0303029"));
        assert_eq!(row(&pool, "f").await.1.as_deref(), Some("cs.CV/9901002"));
    }

    #[tokio::test]
    async fn unique_cite_keys_migration_resuffixes_later_duplicates() {
        let (_dir, pool) = pool_before_migration("unique cite keys").await;
        seed(
            &pool,
            "a",
            "2026-01-01T00:00:00+00:00",
            &[("cite_key", "he2016deep")],
        )
        .await;
        seed(
            &pool,
            "b",
            "2026-01-02T00:00:00+00:00",
            &[("cite_key", "he2016deep")],
        )
        .await;
        seed(
            &pool,
            "c",
            "2026-01-03T00:00:00+00:00",
            &[("cite_key", "he2016deep")],
        )
        .await;
        seed(&pool, "d", "2026-01-04T00:00:00+00:00", &[]).await;
        seed(&pool, "e", "2026-01-05T00:00:00+00:00", &[]).await;
        MIGRATOR.run(&pool).await.unwrap();

        let keys: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT id, cite_key FROM papers ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        let keys: Vec<_> = keys
            .iter()
            .map(|(id, k)| (id.as_str(), k.as_deref()))
            .collect();
        assert_eq!(
            keys,
            [
                ("a", Some("he2016deep")),
                ("b", Some("he2016deep-1")),
                ("c", Some("he2016deep-2")),
                ("d", None),
                ("e", None),
            ]
        );
        let dup = sqlx::query("UPDATE papers SET cite_key = 'he2016deep' WHERE id = 'b'")
            .execute(&pool)
            .await
            .map_err(anyhow::Error::from)
            .unwrap_err();
        assert!(is_unique_violation(&dup));
    }

    #[tokio::test]
    async fn a_change_watch_wakes_on_commits_after_the_last_mark() {
        let (_dir, pool) = temp_pool().await;
        let mut watch = ChangeWatch::open(&pool).await.unwrap();
        let quiet = tokio::time::timeout(Duration::from_millis(2500), watch.changed()).await;
        assert!(quiet.is_err(), "nothing committed yet");

        crate::testutil::insert_test_paper(&pool).await;
        tokio::time::timeout(Duration::from_secs(10), watch.changed())
            .await
            .expect("a commit through the pool is noticed");

        watch.mark_seen().await.unwrap();
        let quiet = tokio::time::timeout(Duration::from_millis(2500), watch.changed()).await;
        assert!(quiet.is_err(), "already seen");
    }
}
