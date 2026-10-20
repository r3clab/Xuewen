use anyhow::Result;
use sqlx::SqlitePool;

use super::model::Paper;

/// Mark a paper as trashed (soft-delete). Returns true if a row was newly
/// trashed (false if it didn't exist or was already trashed).
pub async fn soft_delete(pool: &SqlitePool, id: &str) -> Result<bool> {
    let ts = crate::db::now_rfc3339();
    let res = sqlx::query("UPDATE papers SET deleted_at = ? WHERE id = ? AND deleted_at IS NULL")
        .bind(ts)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Un-trash a paper. Returns true if a row was actually restored.
pub async fn restore(pool: &SqlitePool, id: &str) -> Result<bool> {
    let res =
        sqlx::query("UPDATE papers SET deleted_at = NULL WHERE id = ? AND deleted_at IS NOT NULL")
            .bind(id)
            .execute(pool)
            .await?;
    Ok(res.rows_affected() > 0)
}

/// Every trashed paper, oldest-trashed first.
pub(crate) async fn list(pool: &SqlitePool) -> Result<Vec<Paper>> {
    let papers = sqlx::query_as::<_, Paper>(
        "SELECT * FROM papers WHERE deleted_at IS NOT NULL ORDER BY deleted_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(papers)
}

/// Permanently remove a paper row (the caller removes the PDF file).
pub async fn hard_delete(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM papers WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::model::PaperStatus;
    use crate::library::papers;
    use crate::testutil::{sample_paper, temp_pool};

    #[tokio::test]
    async fn soft_delete_hides_and_purge_removes() {
        let (_dir, pool) = temp_pool().await;
        let mut a = sample_paper("01890000-0000-7000-8000-0000000000a1", "ha");
        a.meta.status = PaperStatus::Resolved;
        let b = sample_paper("01890000-0000-7000-8000-0000000000b2", "hb");
        papers::insert(&pool, &a).await.unwrap();
        papers::insert(&pool, &b).await.unwrap();

        // Soft-delete a: hidden from list/papers::stats/papers::all; b remains.
        assert!(soft_delete(&pool, &a.id).await.unwrap());
        assert!(!soft_delete(&pool, &a.id).await.unwrap()); // idempotent: already trashed
        let listed = papers::list(&pool, None, None, &papers::PaperFilter::default())
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, b.id);
        assert_eq!(papers::stats(&pool).await.unwrap().0, 1); // total counts only active
        assert_eq!(papers::all(&pool).await.unwrap().len(), 1);

        // list sees a.
        let trashed = list(&pool).await.unwrap();
        assert_eq!(trashed.len(), 1);
        assert_eq!(trashed[0].id, a.id);

        // papers::find_one still resolves a trashed paper (by prefix), and papers::get sees it.
        let found = papers::find_one(&pool, "01890000-0000-7000-8000-0000000000a")
            .await
            .unwrap();
        assert_eq!(found.id, a.id);

        // purge (hard_delete) removes it entirely.
        hard_delete(&pool, &a.id).await.unwrap();
        assert!(papers::get(&pool, &a.id).await.unwrap().is_none());
        assert!(list(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn restore_untrashes_only_trashed_rows() {
        let (_dir, pool) = temp_pool().await;
        let p = sample_paper("01890000-0000-7000-8000-000000000003", "h3");
        papers::insert(&pool, &p).await.unwrap();
        assert!(!restore(&pool, &p.id).await.unwrap()); // active: nothing to restore
        soft_delete(&pool, &p.id).await.unwrap();
        assert!(restore(&pool, &p.id).await.unwrap());
        assert!(!restore(&pool, &p.id).await.unwrap()); // idempotent: already active
        assert!(papers::get(&pool, &p.id)
            .await
            .unwrap()
            .unwrap()
            .deleted_at
            .is_none());
        assert_eq!(
            papers::list(&pool, None, None, &papers::PaperFilter::default())
                .await
                .unwrap()
                .len(),
            1
        );
    }
}
