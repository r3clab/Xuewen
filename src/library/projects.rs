use anyhow::{bail, Result};
use serde::Serialize;
use sqlx::QueryBuilder;
use sqlx::SqlitePool;
use std::collections::HashMap;

use super::model::NameError;
use crate::db::now_rfc3339;

/// A named group of related papers. Column names match the `projects` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow, ts_rs::TS)]
#[ts(rename = "ProjectRow")]
pub struct Project {
    pub id: String,
    pub(crate) name: String,
    created_at: String,
}

/// A project plus its membership count, for list views.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow, ts_rs::TS)]
#[ts(export, rename = "Project")]
pub struct ProjectSummary {
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub(crate) project: Project,
    pub paper_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectName(String);

impl ProjectName {
    pub fn parse(raw: &str) -> Result<ProjectName, NameError> {
        let name = raw.trim();
        if name.is_empty() {
            return Err(NameError::Empty);
        }
        Ok(ProjectName(name.to_string()))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

pub async fn create(pool: &SqlitePool, name: &ProjectName) -> Result<Project> {
    let project = Project {
        id: uuid::Uuid::now_v7().to_string(),
        name: name.as_str().to_string(),
        created_at: now_rfc3339(),
    };
    sqlx::query("INSERT INTO projects (id, name, created_at) VALUES (?,?,?)")
        .bind(&project.id)
        .bind(&project.name)
        .bind(&project.created_at)
        .execute(pool)
        .await?;
    Ok(project)
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<ProjectSummary>> {
    // Count only active members: a trashed paper's membership row lingers (until
    // FK cascade on purge), but the `?project=` filter runs under
    // `deleted_at IS NULL`, so the count must match what filtering shows.
    let rows = sqlx::query_as::<_, ProjectSummary>(
        "SELECT p.id, p.name, p.created_at, \
         COUNT(pa.id) AS paper_count \
         FROM projects p \
         LEFT JOIN paper_projects pp ON pp.project_id = p.id \
         LEFT JOIN papers pa ON pa.id = pp.paper_id AND pa.deleted_at IS NULL \
         GROUP BY p.id ORDER BY p.name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub(crate) async fn get(pool: &SqlitePool, id: &str) -> Result<Option<Project>> {
    let p = sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(p)
}

/// Rename a project. `None` keeps the current name (callers map omitted or
/// blank input to `None`). Returns the updated row, or `None` for an unknown id.
pub(crate) async fn update(
    pool: &SqlitePool,
    id: &str,
    name: Option<&ProjectName>,
) -> Result<Option<Project>> {
    let p = sqlx::query_as::<_, Project>(
        "UPDATE projects SET name = COALESCE(?1, name) WHERE id = ?2 \
         RETURNING id, name, created_at",
    )
    .bind(name.map(ProjectName::as_str))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(p)
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<bool> {
    let res = sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn add_paper(pool: &SqlitePool, paper_id: &str, project_id: &str) -> Result<()> {
    let ts = now_rfc3339();
    sqlx::query(
        "INSERT INTO paper_projects (paper_id, project_id, added_at) VALUES (?,?,?) \
         ON CONFLICT (paper_id, project_id) DO NOTHING",
    )
    .bind(paper_id)
    .bind(project_id)
    .bind(&ts)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn remove_paper(pool: &SqlitePool, paper_id: &str, project_id: &str) -> Result<bool> {
    let res = sqlx::query("DELETE FROM paper_projects WHERE paper_id = ? AND project_id = ?")
        .bind(paper_id)
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// A paper's project memberships, ordered by when it joined each project.
pub async fn for_paper(pool: &SqlitePool, paper_id: &str) -> Result<Vec<Project>> {
    let projects = sqlx::query_as::<_, Project>(
        "SELECT p.* FROM projects p JOIN paper_projects pp ON pp.project_id = p.id \
         WHERE pp.paper_id = ? ORDER BY pp.added_at",
    )
    .bind(paper_id)
    .fetch_all(pool)
    .await?;
    Ok(projects)
}

/// Project memberships for many papers in one query, keyed by paper id
/// (papers with none are absent). Per-paper order matches
/// `for_paper`: when the paper joined each project. Per-id binds are
/// fine at library scale (same judgment as `papers_by_ids_ordered`).
pub async fn for_papers(
    pool: &SqlitePool,
    paper_ids: &[String],
) -> Result<HashMap<String, Vec<Project>>> {
    if paper_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut qb: QueryBuilder<sqlx::Sqlite> = QueryBuilder::new(
        "SELECT pp.paper_id, p.id, p.name, p.created_at \
         FROM projects p JOIN paper_projects pp ON pp.project_id = p.id \
         WHERE pp.paper_id IN (",
    );
    let mut sep = qb.separated(", ");
    for id in paper_ids {
        sep.push_bind(id);
    }
    qb.push(") ORDER BY pp.added_at");
    let rows: Vec<(String, String, String, String)> = qb.build_query_as().fetch_all(pool).await?;
    let mut out: HashMap<String, Vec<Project>> = HashMap::new();
    for (paper_id, id, name, created_at) in rows {
        out.entry(paper_id).or_default().push(Project {
            id,
            name,
            created_at,
        });
    }
    Ok(out)
}

pub async fn ids_for_paper(pool: &SqlitePool, paper_id: &str) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT project_id FROM paper_projects WHERE paper_id = ? ORDER BY added_at",
    )
    .bind(paper_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

pub(crate) async fn find_by_name(pool: &SqlitePool, name: &str) -> Result<Option<Project>> {
    let p = sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE name = ? COLLATE NOCASE")
        .bind(name)
        .fetch_optional(pool)
        .await?;
    Ok(p)
}

/// Resolve a project selector: exact (case-insensitive) name, then exact id,
/// then unique id prefix. Errors on no match or an ambiguous prefix.
pub(crate) async fn find_one(pool: &SqlitePool, sel: &str) -> Result<Project> {
    if let Some(p) = find_by_name(pool, sel).await? {
        return Ok(p);
    }
    if let Some(p) = get(pool, sel).await? {
        return Ok(p);
    }
    let pattern = format!("{sel}%");
    let mut matches = sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id LIKE ?")
        .bind(&pattern)
        .fetch_all(pool)
        .await?;
    match matches.len() {
        0 => bail!("no project named or id-prefixed {sel:?}"),
        1 => Ok(matches.pop().unwrap()),
        n => bail!("ambiguous project selector {sel:?} matches {n} projects"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::is_unique_violation;
    use crate::testutil::temp_pool;

    fn name(raw: &str) -> ProjectName {
        ProjectName::parse(raw).unwrap()
    }

    #[tokio::test]
    async fn project_crud_and_unique_name() {
        let (_dir, pool) = temp_pool().await;

        let p = create(&pool, &name("Survey")).await.unwrap();
        assert_eq!(p.name, "Survey");

        // Case-insensitive unique name.
        let dup = create(&pool, &name("survey")).await;
        assert!(dup.is_err());
        assert!(is_unique_violation(&dup.unwrap_err()));

        // List with counts (zero members yet).
        let list = list(&pool).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].project.id, p.id);
        assert_eq!(list[0].paper_count, 0);

        // Update name; RETURNING hands the row back in the same statement.
        let updated = update(&pool, &p.id, Some(&name("Survey v2")))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.name, "Survey v2");
        // `None` keeps the current name.
        let kept = update(&pool, &p.id, None).await.unwrap().unwrap();
        assert_eq!(kept.name, "Survey v2");
        // Unknown id → None.
        assert!(update(&pool, "nope", Some(&name("X")))
            .await
            .unwrap()
            .is_none());

        // Delete.
        assert!(delete(&pool, &p.id).await.unwrap());
        assert!(get(&pool, &p.id).await.unwrap().is_none());
        assert!(!delete(&pool, &p.id).await.unwrap());
    }

    #[tokio::test]
    async fn find_one_project_by_name_then_prefix() {
        let (_dir, pool) = temp_pool().await;
        let p = create(&pool, &name("My Survey")).await.unwrap();

        // Exact, case-insensitive name.
        assert_eq!(find_one(&pool, "my survey").await.unwrap().id, p.id);
        // Id prefix.
        assert_eq!(find_one(&pool, &p.id[..8]).await.unwrap().id, p.id);
        // Miss.
        assert!(find_one(&pool, "nope").await.is_err());
    }
}
