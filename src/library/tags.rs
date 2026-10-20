use anyhow::Result;
use serde::Serialize;
use sqlx::QueryBuilder;
use sqlx::SqlitePool;
use std::collections::HashMap;

use super::model::NameError;
use crate::db::now_rfc3339;

/// A topical label. Column names match the `tags` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow, ts_rs::TS)]
#[ts(rename = "TagRow")]
pub struct Tag {
    pub id: String,
    pub name: String,
    created_at: String,
}

/// A tag plus how many papers carry it (or any of its `name/*` children).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow, ts_rs::TS)]
#[ts(export)]
pub(crate) struct TagSummary {
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub(crate) tag: Tag,
    pub(crate) paper_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagName(String);

impl TagName {
    pub fn parse(raw: &str) -> Result<TagName, NameError> {
        normalize_tag_name(raw).map(TagName).ok_or(NameError::Empty)
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

/// Split on `/`, trim each segment, drop empties, rejoin. `None` if empty.
fn normalize_tag_name(raw: &str) -> Option<String> {
    let joined = raw
        .split('/')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    (!joined.is_empty()).then_some(joined)
}

pub(crate) async fn find_by_name(pool: &SqlitePool, name: &str) -> Result<Option<Tag>> {
    let t = sqlx::query_as::<_, Tag>("SELECT * FROM tags WHERE name = ? COLLATE NOCASE")
        .bind(name)
        .fetch_optional(pool)
        .await?;
    Ok(t)
}

/// Create the tag if it doesn't exist (case-insensitively), then link it to
/// the paper. Idempotent: re-adding an already-linked tag is a no-op.
pub async fn add_paper(pool: &SqlitePool, paper_id: &str, name: &TagName) -> Result<Tag> {
    // Atomic get-or-create: a find-then-create pair races on the NOCASE unique
    // index when two adds of the same new name run concurrently, and the loser
    // surfaces a UNIQUE violation. `DO UPDATE SET name = name` is a no-op write
    // whose RETURNING yields the existing row (its casing, its created_at).
    let tag = sqlx::query_as::<_, Tag>(
        "INSERT INTO tags (id, name, created_at) VALUES (?,?,?) \
         ON CONFLICT(name COLLATE NOCASE) DO UPDATE SET name = name \
         RETURNING id, name, created_at",
    )
    .bind(uuid::Uuid::now_v7().to_string())
    .bind(name.as_str())
    .bind(now_rfc3339())
    .fetch_one(pool)
    .await?;
    let ts = now_rfc3339();
    sqlx::query(
        "INSERT INTO paper_tags (paper_id, tag_id, added_at) VALUES (?,?,?) \
         ON CONFLICT (paper_id, tag_id) DO NOTHING",
    )
    .bind(paper_id)
    .bind(&tag.id)
    .bind(&ts)
    .execute(pool)
    .await?;
    Ok(tag)
}

/// Unlink a tag from a paper; if that was the tag's last membership, delete
/// the now-orphaned tag row too.
pub(crate) async fn remove_paper(pool: &SqlitePool, paper_id: &str, tag_id: &str) -> Result<bool> {
    let removed = sqlx::query("DELETE FROM paper_tags WHERE paper_id = ? AND tag_id = ?")
        .bind(paper_id)
        .bind(tag_id)
        .execute(pool)
        .await?
        .rows_affected()
        > 0;
    // GC in one atomic statement: a COUNT-then-DELETE pair would let a
    // concurrent add link this tag between the two, and the tag DELETE's
    // cascade on paper_tags would then destroy that fresh membership.
    sqlx::query(
        "DELETE FROM tags WHERE id = ?1 \
         AND NOT EXISTS (SELECT 1 FROM paper_tags WHERE tag_id = ?1)",
    )
    .bind(tag_id)
    .execute(pool)
    .await?;
    Ok(removed)
}

pub async fn for_paper(pool: &SqlitePool, paper_id: &str) -> Result<Vec<Tag>> {
    let tags = sqlx::query_as::<_, Tag>(
        "SELECT t.* FROM tags t JOIN paper_tags pt ON pt.tag_id = t.id \
         WHERE pt.paper_id = ? ORDER BY t.name COLLATE NOCASE",
    )
    .bind(paper_id)
    .fetch_all(pool)
    .await?;
    Ok(tags)
}

/// Tag memberships for many papers in one query, keyed by paper id (papers
/// with none are absent). Per-paper order matches `for_paper`: tag name,
/// case-insensitively.
pub async fn for_papers(
    pool: &SqlitePool,
    paper_ids: &[String],
) -> Result<HashMap<String, Vec<Tag>>> {
    if paper_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut qb: QueryBuilder<sqlx::Sqlite> = QueryBuilder::new(
        "SELECT pt.paper_id, t.id, t.name, t.created_at \
         FROM tags t JOIN paper_tags pt ON pt.tag_id = t.id \
         WHERE pt.paper_id IN (",
    );
    let mut sep = qb.separated(", ");
    for id in paper_ids {
        sep.push_bind(id);
    }
    qb.push(") ORDER BY t.name COLLATE NOCASE");
    let rows: Vec<(String, String, String, String)> = qb.build_query_as().fetch_all(pool).await?;
    let mut out: HashMap<String, Vec<Tag>> = HashMap::new();
    for (paper_id, id, name, created_at) in rows {
        out.entry(paper_id).or_default().push(Tag {
            id,
            name,
            created_at,
        });
    }
    Ok(out)
}

pub(crate) async fn list(pool: &SqlitePool) -> Result<Vec<TagSummary>> {
    // `paper_count` is a prefix rollup, not a flat count: it counts distinct
    // papers carrying this tag OR any `name/*` child, matching what clicking the
    // tag's filter returns (parent + every descendant).
    // NOTE: a tag name literally containing `%`/`_` would over-match under LIKE;
    // this mirrors the search filter's known caveat — we don't escape here.
    let rows = sqlx::query_as::<_, TagSummary>(
        "SELECT t.id, t.name, t.created_at, \
                (SELECT COUNT(DISTINCT pt.paper_id) \
                   FROM paper_tags pt JOIN tags c ON c.id = pt.tag_id \
                   WHERE c.name = t.name OR c.name LIKE t.name || '/%') AS paper_count \
         FROM tags t ORDER BY t.name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub(crate) async fn rename(pool: &SqlitePool, id: &str, name: &TagName) -> Result<Option<Tag>> {
    let tag = sqlx::query_as::<_, Tag>(
        "UPDATE tags SET name = ? WHERE id = ? RETURNING id, name, created_at",
    )
    .bind(name.as_str())
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(tag)
}

pub(crate) async fn delete(pool: &SqlitePool, id: &str) -> Result<bool> {
    let res = sqlx::query("DELETE FROM tags WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{insert_test_paper, temp_pool};

    fn tag(raw: &str) -> TagName {
        TagName::parse(raw).unwrap()
    }

    #[test]
    fn normalize_trims_and_drops_empty_segments() {
        assert_eq!(
            normalize_tag_name(" security / fuzzing "),
            Some("security/fuzzing".into())
        );
        assert_eq!(normalize_tag_name("ml//llm/"), Some("ml/llm".into()));
        assert_eq!(normalize_tag_name("  "), None);
        assert_eq!(normalize_tag_name("//"), None);
    }

    #[tokio::test]
    async fn add_tag_creates_then_reuses_case_insensitively() {
        let (_dir, pool) = temp_pool().await;
        let pid = insert_test_paper(&pool).await;
        let a = add_paper(&pool, &pid, &tag("Security/Fuzzing"))
            .await
            .unwrap();
        let b = add_paper(&pool, &pid, &tag("security/fuzzing"))
            .await
            .unwrap();
        assert_eq!(a.id, b.id, "same tag reused case-insensitively");
        assert_eq!(b.name, "Security/Fuzzing", "existing casing wins");
        assert_eq!(b.created_at, a.created_at);
        assert_eq!(list(&pool).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn remove_keeps_tag_while_memberships_remain() {
        let (_dir, pool) = temp_pool().await;
        let p1 = insert_test_paper(&pool).await;
        let p2 = insert_test_paper(&pool).await;
        let t = add_paper(&pool, &p1, &tag("ml")).await.unwrap();
        add_paper(&pool, &p2, &tag("ml")).await.unwrap();
        assert!(remove_paper(&pool, &p1, &t.id).await.unwrap());
        assert!(
            find_by_name(&pool, "ml").await.unwrap().is_some(),
            "tag survives while another paper still carries it"
        );
    }

    #[tokio::test]
    async fn remove_last_membership_gcs_the_tag() {
        let (_dir, pool) = temp_pool().await;
        let pid = insert_test_paper(&pool).await;
        let t = add_paper(&pool, &pid, &tag("ml")).await.unwrap();
        assert!(remove_paper(&pool, &pid, &t.id).await.unwrap());
        assert!(
            find_by_name(&pool, "ml").await.unwrap().is_none(),
            "orphan tag GC'd"
        );
    }

    #[tokio::test]
    async fn tag_counts_roll_up_children() {
        let (_dir, pool) = temp_pool().await;
        // One paper carries a child tag `security/fuzzing`; another carries the
        // bare parent `security`.
        let child_paper = insert_test_paper(&pool).await;
        let parent_paper = insert_test_paper(&pool).await;
        add_paper(&pool, &child_paper, &tag("security/fuzzing"))
            .await
            .unwrap();
        add_paper(&pool, &parent_paper, &tag("security"))
            .await
            .unwrap();

        let counts = list(&pool).await.unwrap();
        let by_name = |n: &str| counts.iter().find(|s| s.tag.name == n).unwrap().paper_count;
        // `security` rolls up itself + every `security/*` child paper.
        assert_eq!(by_name("security"), 2);
        // The leaf child is unaffected: its rollup equals its direct count.
        assert_eq!(by_name("security/fuzzing"), 1);
    }
}
