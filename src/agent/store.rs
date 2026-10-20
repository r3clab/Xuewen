//! Persistence for a paper's attached code repository (`paper_code`): the
//! clone-generation state machine `code.rs` drives and the Ask tab reads
//! before exposing `repo/` to the agent.

use anyhow::Result;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::db::now_rfc3339;

/// `paper_code.status` — the schema CHECK-constrains exactly these values
/// (`0017_add_agent.sql`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type, ts_rs::TS)]
#[sqlx(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub(crate) enum CodeStatus {
    Cloning,
    Ready,
    Error,
}

/// The wire/schema string (`xuewen code status` prints it).
impl std::fmt::Display for CodeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            CodeStatus::Cloning => "cloning",
            CodeStatus::Ready => "ready",
            CodeStatus::Error => "error",
        })
    }
}

/// A paper's attached code repository (`paper_code` row).
#[derive(Debug, Clone, Serialize, sqlx::FromRow, ts_rs::TS)]
#[ts(export, rename = "PaperCodeStatus")]
pub struct PaperCode {
    paper_id: String,
    pub(crate) repo_url: String,
    pub(crate) commit_sha: Option<String>,
    pub(crate) status: CodeStatus,
    pub(crate) error: Option<String>,
    cloned_at: Option<String>,
    size_bytes: Option<i64>,
}

pub async fn get_paper_code(pool: &SqlitePool, paper_id: &str) -> Result<Option<PaperCode>> {
    Ok(sqlx::query_as::<_, PaperCode>(
        "SELECT paper_id, repo_url, commit_sha, status, error, cloned_at, size_bytes
         FROM paper_code WHERE paper_id = ?",
    )
    .bind(paper_id)
    .fetch_optional(pool)
    .await?)
}

/// Attach (or re-attach) a repo: the row enters 'cloning' with outcome
/// fields cleared; the background job resolves it to ready/error.
/// Mark a paper's repo as (re)cloning and return the new `clone_gen`. Each
/// call bumps the generation so a background clone can detect it has been
/// superseded by a later attach (see `set_paper_code_ready`/`_error`).
pub async fn upsert_paper_code_cloning(
    pool: &SqlitePool,
    paper_id: &str,
    repo_url: &str,
) -> Result<i64> {
    let gen: i64 = sqlx::query_scalar(
        "INSERT INTO paper_code (paper_id, repo_url, status, clone_gen) VALUES (?, ?, 'cloning', 0)
         ON CONFLICT(paper_id) DO UPDATE SET repo_url = excluded.repo_url, status = 'cloning',
           commit_sha = NULL, error = NULL, cloned_at = NULL, size_bytes = NULL,
           clone_gen = paper_code.clone_gen + 1
         RETURNING clone_gen",
    )
    .bind(paper_id)
    .bind(repo_url)
    .fetch_one(pool)
    .await?;
    Ok(gen)
}

/// The current `clone_gen` for a paper, or `None` if no row exists. A clone job
/// consults this before publishing its checkout so a superseded job bows out.
pub(crate) async fn current_clone_gen(pool: &SqlitePool, paper_id: &str) -> Result<Option<i64>> {
    Ok(
        sqlx::query_scalar("SELECT clone_gen FROM paper_code WHERE paper_id = ?")
            .bind(paper_id)
            .fetch_optional(pool)
            .await?,
    )
}

/// Record a successful clone, but only if `clone_gen` still matches — a later
/// attach bumps it, so a stale job's write affects no rows. Returns whether the
/// row was updated (i.e. this job was still the current one).
pub(crate) async fn set_paper_code_ready(
    pool: &SqlitePool,
    paper_id: &str,
    commit_sha: &str,
    size_bytes: i64,
    clone_gen: i64,
) -> Result<bool> {
    let r = sqlx::query(
        "UPDATE paper_code SET status = 'ready', commit_sha = ?, size_bytes = ?,
           cloned_at = ?, error = NULL WHERE paper_id = ? AND clone_gen = ?",
    )
    .bind(commit_sha)
    .bind(size_bytes)
    .bind(now_rfc3339())
    .bind(paper_id)
    .bind(clone_gen)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

/// Record a clone failure, guarded by `clone_gen` like `set_paper_code_ready`.
/// Returns whether the row was updated.
pub(crate) async fn set_paper_code_error(
    pool: &SqlitePool,
    paper_id: &str,
    error: &str,
    clone_gen: i64,
) -> Result<bool> {
    let r = sqlx::query(
        "UPDATE paper_code SET status = 'error', error = ? WHERE paper_id = ? AND clone_gen = ?",
    )
    .bind(error)
    .bind(paper_id)
    .bind(clone_gen)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

/// Boot-time sweep: a row still 'cloning' with no live owner was interrupted
/// mid-clone by a shutdown or crash, and its status could otherwise never
/// resolve. `skip_paper_ids` names the papers whose clone a sibling process
/// (CLI `xuewen code set`, the desktop app) is still running — the boot must
/// not fail those out from under it. The generation is bumped so a job this
/// sweep failed anyway (a liveness misread) has its late ready/error writes
/// miss the `WHERE clone_gen = ?` guard instead of overwriting this outcome.
/// Returns the number of rows fixed.
pub(crate) async fn fail_interrupted_clones(
    pool: &SqlitePool,
    skip_paper_ids: &[String],
) -> Result<u64> {
    let mut qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new(
        "UPDATE paper_code SET status = 'error',
           error = 'interrupted by a server restart — re-attach to retry',
           clone_gen = clone_gen + 1
         WHERE status = 'cloning'",
    );
    if !skip_paper_ids.is_empty() {
        qb.push(" AND paper_id NOT IN (");
        let mut sep = qb.separated(", ");
        for id in skip_paper_ids {
            sep.push_bind(id);
        }
        qb.push(")");
    }
    let r = qb.build().execute(pool).await?;
    Ok(r.rows_affected())
}

pub(crate) async fn delete_paper_code(pool: &SqlitePool, paper_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM paper_code WHERE paper_id = ?")
        .bind(paper_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::pool_with_paper;

    #[test]
    fn status_strings_match_schema() {
        for (status, want) in [
            (CodeStatus::Cloning, "\"cloning\""),
            (CodeStatus::Ready, "\"ready\""),
            (CodeStatus::Error, "\"error\""),
        ] {
            assert_eq!(serde_json::to_string(&status).unwrap(), want);
        }
    }

    #[tokio::test]
    async fn paper_code_lifecycle() {
        let (pool, _dir) = pool_with_paper("p1").await;
        assert!(get_paper_code(&pool, "p1").await.unwrap().is_none());

        let gen0 = upsert_paper_code_cloning(&pool, "p1", "https://github.com/x/y")
            .await
            .unwrap();
        assert_eq!(gen0, 0);
        let c = get_paper_code(&pool, "p1").await.unwrap().unwrap();
        assert_eq!(c.status, CodeStatus::Cloning);
        assert_eq!(c.repo_url, "https://github.com/x/y");
        assert_eq!(current_clone_gen(&pool, "p1").await.unwrap(), Some(0));

        assert!(set_paper_code_ready(&pool, "p1", "abc1234", 42_000, gen0)
            .await
            .unwrap());
        let c = get_paper_code(&pool, "p1").await.unwrap().unwrap();
        assert_eq!(c.status, CodeStatus::Ready);
        assert_eq!(c.commit_sha.as_deref(), Some("abc1234"));
        // RFC3339 like every other timestamp column, not `datetime('now')`.
        let cloned_at = c.cloned_at.as_deref().unwrap();
        assert!(
            chrono::DateTime::parse_from_rfc3339(cloned_at).is_ok(),
            "cloned_at not RFC3339: {cloned_at}"
        );

        // Re-attach resets to cloning, clears the old outcome fields, and bumps
        // the generation.
        let gen1 = upsert_paper_code_cloning(&pool, "p1", "https://github.com/x/z")
            .await
            .unwrap();
        assert_eq!(gen1, 1);
        let c = get_paper_code(&pool, "p1").await.unwrap().unwrap();
        assert_eq!(c.status, CodeStatus::Cloning);
        assert_eq!(c.commit_sha, None);

        // A write guarded by the stale generation is dropped (0 rows), while
        // the current generation's write lands.
        assert!(!set_paper_code_error(&pool, "p1", "stale", gen0)
            .await
            .unwrap());
        assert!(set_paper_code_error(&pool, "p1", "boom", gen1)
            .await
            .unwrap());
        assert_eq!(
            get_paper_code(&pool, "p1")
                .await
                .unwrap()
                .unwrap()
                .error
                .as_deref(),
            Some("boom")
        );

        delete_paper_code(&pool, "p1").await.unwrap();
        assert!(get_paper_code(&pool, "p1").await.unwrap().is_none());
        assert_eq!(current_clone_gen(&pool, "p1").await.unwrap(), None);
    }
}
