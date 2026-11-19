use anyhow::{bail, Result};
use sqlx::QueryBuilder;
use sqlx::SqlitePool;
use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::model::{NameError, Paper, PaperStatus};
use super::tags;
use crate::db::escape_like;

/// Longest accepted manual name — a short identifier like "RVSpec", not prose.
const NAME_MAX_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PaperName(String);

impl PaperName {
    pub(crate) fn parse(raw: &str) -> Result<Option<PaperName>, NameError> {
        let name = raw.trim();
        if name.is_empty() {
            return Ok(None);
        }
        if name.chars().count() > NAME_MAX_CHARS {
            return Err(NameError::TooLong {
                max: NAME_MAX_CHARS,
            });
        }
        // The name renders in single-line chips/cells; an embedded newline or
        // other control character would corrupt every one of them.
        if name.chars().any(char::is_control) {
            return Err(NameError::Invalid);
        }
        Ok(Some(PaperName(name.to_string())))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// A write tried to claim a cite key another paper holds.
#[derive(Debug)]
pub(crate) struct CiteKeyTaken;

impl std::fmt::Display for CiteKeyTaken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cite key already taken")
    }
}

impl std::error::Error for CiteKeyTaken {}

pub(crate) fn is_cite_key_taken(e: &anyhow::Error) -> bool {
    e.downcast_ref::<CiteKeyTaken>().is_some()
}

/// The paper (active or trashed) whose stored bytes match `content_hash`.
pub(crate) async fn find_by_hash(pool: &SqlitePool, content_hash: &str) -> Result<Option<Paper>> {
    let p = sqlx::query_as::<_, Paper>("SELECT * FROM papers WHERE content_hash = ?")
        .bind(content_hash)
        .fetch_optional(pool)
        .await?;
    Ok(p)
}

/// The paper (active or trashed) other than `exclude_id` already holding
/// `doi` or `arxiv_id`, both in canonical form (`model::canonical_doi`/
/// `canonical_arxiv`), as every stored identifier is. A DOI match wins over
/// an arXiv match when both exist.
pub(crate) async fn find_by_identifier(
    pool: &SqlitePool,
    doi: Option<&str>,
    arxiv_id: Option<&str>,
    exclude_id: Option<&str>,
) -> Result<Option<Paper>> {
    let p = sqlx::query_as::<_, Paper>(
        "SELECT * FROM papers WHERE (doi = ?1 OR arxiv_id = ?2) AND id IS NOT ?3 \
         ORDER BY doi IS ?1 DESC LIMIT 1",
    )
    .bind(doi)
    .bind(arxiv_id)
    .bind(exclude_id)
    .fetch_optional(pool)
    .await?;
    Ok(p)
}

/// Insert `p`; fails with `CiteKeyTaken` when another paper holds its cite key.
pub async fn insert(pool: &SqlitePool, p: &Paper) -> Result<()> {
    let inserted = sqlx::query(
        "INSERT INTO papers \
         (id, content_hash, rel_path, title, abstract, authors, venue, year, \
          doi, arxiv_id, dblp_key, cite_key, url, source, status, added_at, \
          deleted_at, starred, name) \
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) \
         ON CONFLICT (cite_key) DO NOTHING",
    )
    .bind(&p.id)
    .bind(&p.content_hash)
    .bind(&p.rel_path)
    .bind(&p.meta.title)
    .bind(&p.meta.abstract_text)
    .bind(&p.meta.authors)
    .bind(&p.meta.venue)
    .bind(p.meta.year)
    .bind(&p.meta.doi)
    .bind(&p.meta.arxiv_id)
    .bind(&p.meta.dblp_key)
    .bind(&p.cite_key)
    .bind(&p.meta.url)
    .bind(&p.meta.source)
    .bind(p.meta.status)
    .bind(&p.added_at)
    .bind(&p.deleted_at)
    .bind(p.starred)
    .bind(&p.name)
    .execute(pool)
    .await?
    .rows_affected()
        > 0;
    if !inserted {
        bail!(CiteKeyTaken);
    }
    Ok(())
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<Paper>> {
    let p = sqlx::query_as::<_, Paper>("SELECT * FROM papers WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(p)
}

/// Cite keys other papers hold that extend `base` — every key
/// `naming::disambiguate` could pick from it is among them. `exclude_id`
/// skips a paper's own key (used when re-filing during refresh).
pub(crate) async fn cite_keys_with_base(
    pool: &SqlitePool,
    base: &str,
    exclude_id: Option<&str>,
) -> Result<HashSet<String>> {
    // A range over the unique index rather than `LIKE 'base%'`, which cannot
    // use it: U+10FFFF sorts after any character a key can continue with.
    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT cite_key FROM papers \
         WHERE cite_key >= ?1 AND cite_key < ?1 || char(0x10FFFF) AND id IS NOT ?2",
    )
    .bind(base)
    .bind(exclude_id)
    .fetch_all(pool)
    .await?;
    Ok(keys.into_iter().collect())
}

/// Overwrite an active paper's metadata block and filing (`rel_path`,
/// `cite_key`): the columns identify and refresh own. Every other column has
/// another owner — `trash` the deletion mark, `set_starred`/`set_name` theirs
/// — and is never written here. Returns false, writing nothing, when the
/// paper is trashed or gone; fails with `CiteKeyTaken` when another paper
/// holds `p.cite_key`.
pub(crate) async fn update(pool: &SqlitePool, p: &Paper) -> Result<bool> {
    let updated = sqlx::query(
        "UPDATE papers SET \
         rel_path = ?2, title = ?3, abstract = ?4, authors = ?5, venue = ?6, year = ?7, \
         doi = ?8, arxiv_id = ?9, dblp_key = ?10, cite_key = ?11, url = ?12, source = ?13, \
         status = ?14 \
         WHERE id = ?1 AND deleted_at IS NULL \
         AND NOT EXISTS (SELECT 1 FROM papers o WHERE o.cite_key = ?11 AND o.id <> ?1)",
    )
    .bind(&p.id)
    .bind(&p.rel_path)
    .bind(&p.meta.title)
    .bind(&p.meta.abstract_text)
    .bind(&p.meta.authors)
    .bind(&p.meta.venue)
    .bind(p.meta.year)
    .bind(&p.meta.doi)
    .bind(&p.meta.arxiv_id)
    .bind(&p.meta.dblp_key)
    .bind(&p.cite_key)
    .bind(&p.meta.url)
    .bind(&p.meta.source)
    .bind(p.meta.status)
    .execute(pool)
    .await?
    .rows_affected()
        > 0;
    if updated {
        return Ok(true);
    }
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM papers WHERE id = ? AND deleted_at IS NULL)",
    )
    .bind(&p.id)
    .fetch_one(pool)
    .await?;
    if active {
        bail!(CiteKeyTaken);
    }
    Ok(false)
}

/// Every paper, oldest first.
pub(crate) async fn all(pool: &SqlitePool) -> Result<Vec<Paper>> {
    let papers = sqlx::query_as::<_, Paper>(
        "SELECT * FROM papers WHERE deleted_at IS NULL ORDER BY added_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(papers)
}

/// Papers whose id starts with `prefix` (for `refresh <ID>` prefix matching).
async fn find_by_prefix(pool: &SqlitePool, prefix: &str) -> Result<Vec<Paper>> {
    let papers = sqlx::query_as::<_, Paper>("SELECT * FROM papers WHERE id LIKE ? ESCAPE '\\'")
        .bind(format!("{}%", escape_like(prefix)))
        .fetch_all(pool)
        .await?;
    Ok(papers)
}

/// Set (or clear) a paper's starred flag. Returns true if a row was updated.
pub async fn set_starred(pool: &SqlitePool, paper_id: &str, starred: bool) -> Result<bool> {
    Ok(sqlx::query("UPDATE papers SET starred = ? WHERE id = ?")
        .bind(starred as i64)
        .bind(paper_id)
        .execute(pool)
        .await?
        .rows_affected()
        > 0)
}

/// Set (or clear, with `None`) a paper's manual name. Identify/refresh never
/// touch this column (`update` deliberately omits it — see
/// `model::Paper::name`). Returns true if a row was updated.
pub(crate) async fn set_name(
    pool: &SqlitePool,
    paper_id: &str,
    name: Option<&PaperName>,
) -> Result<bool> {
    Ok(sqlx::query("UPDATE papers SET name = ? WHERE id = ?")
        .bind(name.map(PaperName::as_str))
        .bind(paper_id)
        .execute(pool)
        .await?
        .rows_affected()
        > 0)
}

/// Find a paper by exact id, else by unique id prefix (active or trashed).
pub(crate) async fn find_one(pool: &SqlitePool, id: &str) -> Result<Paper> {
    if let Some(p) = get(pool, id).await? {
        return Ok(p);
    }
    let mut matches = find_by_prefix(pool, id).await?;
    match matches.len() {
        0 => bail!("no paper with id or prefix {id:?}"),
        1 => Ok(matches.pop().unwrap()),
        n => bail!("ambiguous id prefix {id:?} matches {n} papers"),
    }
}

/// The status/project/tag/starred restrictions shared by the paper-list and
/// search endpoints. One struct + one SQL emitter (`push_paper_filters`) so
/// `/api/papers` and `/api/search` can never disagree on filter semantics.
/// `q` is deliberately absent: it is a LIKE filter on the list path but
/// Tantivy's job on the search path.
#[derive(Debug, Clone, Default)]
pub struct PaperFilter {
    pub status: Option<PaperStatus>,
    /// Project id.
    pub project: Option<String>,
    /// Matches the tag or any `name/*` descendant, case-insensitively like
    /// tag names themselves.
    pub tag: Option<String>,
    /// Only restricts when `Some(true)`.
    pub starred: Option<bool>,
}

/// Append `filter`'s restrictions to a query already inside its WHERE clause.
pub(crate) fn push_paper_filters(qb: &mut QueryBuilder<sqlx::Sqlite>, filter: &PaperFilter) {
    if let Some(status) = filter.status {
        qb.push(" AND status = ").push_bind(status);
    }
    if let Some(pid) = filter
        .project
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        qb.push(" AND id IN (SELECT paper_id FROM paper_projects WHERE project_id = ")
            .push_bind(pid.to_string())
            .push(")");
    }
    if let Some(tag) = filter
        .tag
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        qb.push(
            " AND id IN (SELECT pt.paper_id FROM paper_tags pt \
                 JOIN tags t ON t.id = pt.tag_id CROSS JOIN (SELECT ",
        )
        .push_bind(tag.to_string())
        .push(" AS name) root WHERE ")
        .push(tags::subtree_sql("t.name", "root.name"))
        .push(")");
    }
    if filter.starred == Some(true) {
        qb.push(" AND starred = 1");
    }
}

/// List papers with optional case-insensitive search (`q` over title+authors),
/// restricted by `filter`.
pub async fn list(
    pool: &SqlitePool,
    q: Option<&str>,
    sort: Sort,
    filter: &PaperFilter,
) -> Result<Vec<Paper>> {
    let mut qb: QueryBuilder<sqlx::Sqlite> =
        QueryBuilder::new("SELECT * FROM papers WHERE deleted_at IS NULL");
    if let Some(term) = q.map(str::trim).filter(|s| !s.is_empty()) {
        let like = format!("%{}%", escape_like(term));
        qb.push(" AND (title LIKE ")
            .push_bind(like.clone())
            .push(" ESCAPE '\\' OR authors LIKE ")
            .push_bind(like)
            .push(" ESCAPE '\\')");
    }
    push_paper_filters(&mut qb, filter);
    qb.push(" ORDER BY ").push(order_by(sort));
    let papers = qb.build_query_as::<Paper>().fetch_all(pool).await?;
    Ok(papers)
}

/// The orders `list` offers.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS, clap::ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
#[ts(export)]
pub enum Sort {
    #[default]
    YearDesc,
    YearAsc,
    AddedDesc,
    Title,
    Name,
}

fn order_by(sort: Sort) -> &'static str {
    match sort {
        Sort::YearDesc => "year DESC",
        Sort::YearAsc => "year ASC NULLS LAST",
        Sort::AddedDesc => "added_at DESC",
        Sort::Title => "title COLLATE NOCASE ASC",
        // NULLS LAST (unlike title's arm): most papers have no name, and a
        // name sort that buried every named paper below the unnamed tail
        // would be useless. Title breaks ties so the tail stays stable.
        Sort::Name => "name COLLATE NOCASE ASC NULLS LAST, title COLLATE NOCASE ASC",
    }
}

/// The active papers among `ids`, in the order `ids` names them.
pub(crate) async fn by_ids(pool: &SqlitePool, ids: &[String]) -> Result<Vec<Paper>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut qb: QueryBuilder<sqlx::Sqlite> =
        QueryBuilder::new("SELECT * FROM papers WHERE deleted_at IS NULL AND id IN (");
    let mut ids_sql = qb.separated(", ");
    for id in ids {
        ids_sql.push_bind(id);
    }
    qb.push(")");
    let mut found = qb.build_query_as::<Paper>().fetch_all(pool).await?;
    let rank: std::collections::HashMap<&str, usize> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    found.sort_by_key(|p| rank[p.id.as_str()]);
    Ok(found)
}

/// `(total, resolved, needs_review)` paper counts.
pub(crate) async fn stats(pool: &SqlitePool) -> Result<(i64, i64, i64)> {
    let row: (i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), \
         COALESCE(SUM(status = 'resolved'), 0), \
         COALESCE(SUM(status = 'needs_review'), 0) \
         FROM papers WHERE deleted_at IS NULL",
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::model::{Authors, PaperStatus};
    use crate::library::trash;
    use crate::testutil::{insert_test_paper, sample_paper, temp_pool};

    #[test]
    fn every_sort_has_its_own_order_and_one_spelling() {
        use clap::ValueEnum;
        let sorts = Sort::value_variants();
        let orders: HashSet<&str> = sorts.iter().map(|s| order_by(*s)).collect();
        assert_eq!(orders.len(), sorts.len());
        for sort in sorts {
            let cli = sort.to_possible_value().unwrap();
            let wire = serde_json::to_value(sort).unwrap();
            assert_eq!(wire, cli.get_name(), "{sort:?}");
        }
    }

    #[tokio::test]
    async fn insert_then_fetch_and_dedup() {
        let (_dir, pool) = temp_pool().await;

        assert!(find_by_hash(&pool, "abc").await.unwrap().is_none());

        let p = sample_paper("01890000-0000-7000-8000-000000000000", "abc");
        insert(&pool, &p).await.unwrap();

        assert!(find_by_hash(&pool, "abc").await.unwrap().is_some());

        let got = get(&pool, &p.id).await.unwrap().unwrap();
        assert_eq!(got.content_hash, "abc");
        assert_eq!(got.meta.title.as_deref(), Some("A Title"));
        assert_eq!(got.meta.status, PaperStatus::NeedsReview);
    }

    #[tokio::test]
    async fn cite_keys_with_base_returns_prefix_matches() {
        let (_dir, pool) = temp_pool().await;

        let mut a = sample_paper("01890000-0000-7000-8000-00000000000a", "ha");
        a.cite_key = Some("he2016deep".into());
        insert(&pool, &a).await.unwrap();

        let mut b = sample_paper("01890000-0000-7000-8000-00000000000b", "hb");
        b.cite_key = Some("he2016deepa".into());
        insert(&pool, &b).await.unwrap();

        let taken = cite_keys_with_base(&pool, "he2016deep", None)
            .await
            .unwrap();
        assert!(taken.contains("he2016deep"));
        assert!(taken.contains("he2016deepa"));

        let taken_excl = cite_keys_with_base(&pool, "he2016deep", Some(&a.id))
            .await
            .unwrap();
        assert!(!taken_excl.contains("he2016deep"));
        assert!(taken_excl.contains("he2016deepa"));

        // Both keys sort just below this base, but neither extends it.
        assert!(cite_keys_with_base(&pool, "he2016deeq", None)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn a_cite_key_names_one_paper() {
        let (_dir, pool) = temp_pool().await;
        let mut a = sample_paper("01890000-0000-7000-8000-00000000001a", "ha");
        a.cite_key = Some("he2016deep".into());
        insert(&pool, &a).await.unwrap();

        let mut b = sample_paper("01890000-0000-7000-8000-00000000001b", "hb");
        b.cite_key = Some("he2016deep".into());
        assert!(is_cite_key_taken(&insert(&pool, &b).await.unwrap_err()));
        b.cite_key = Some("he2016deepa".into());
        insert(&pool, &b).await.unwrap();

        b.cite_key = Some("he2016deep".into());
        assert!(is_cite_key_taken(&update(&pool, &b).await.unwrap_err()));
        let kept = get(&pool, &b.id).await.unwrap().unwrap();
        assert_eq!(kept.cite_key.as_deref(), Some("he2016deepa"));

        // Papers without a key never conflict.
        let c = sample_paper("01890000-0000-7000-8000-00000000001c", "hc");
        let d = sample_paper("01890000-0000-7000-8000-00000000001d", "hd");
        insert(&pool, &c).await.unwrap();
        insert(&pool, &d).await.unwrap();
        assert!(update(&pool, &d).await.unwrap());
    }

    #[tokio::test]
    async fn update_paper_persists_changes() {
        let (_dir, pool) = temp_pool().await;
        let mut p = sample_paper("01890000-0000-7000-8000-0000000000c1", "h1");
        insert(&pool, &p).await.unwrap();

        // Mutate every updatable column to catch a dropped SET clause.
        p.meta.title = Some("New Title".into());
        p.meta.abstract_text = Some("New abstract".into());
        p.meta.authors = Authors(vec!["Ada Lovelace".into()]);
        p.meta.venue = Some("KDD".into());
        p.meta.year = Some(2019);
        p.meta.doi = Some("10.1/new".into());
        p.meta.arxiv_id = Some("2001.00001".into());
        p.meta.dblp_key = Some("conf/kdd/X".into());
        p.rel_path = "he2016deep.pdf".into();
        p.cite_key = Some("he2016deep".into());
        p.meta.url = Some("https://example.org/x".into());
        p.meta.source = Some("crossref".into());
        p.meta.status = PaperStatus::Resolved;
        assert!(update(&pool, &p).await.unwrap());

        let got = get(&pool, &p.id).await.unwrap().unwrap();
        assert_eq!(got.meta.title.as_deref(), Some("New Title"));
        assert_eq!(got.meta.abstract_text.as_deref(), Some("New abstract"));
        assert_eq!(got.meta.authors, Authors(vec!["Ada Lovelace".into()]));
        assert_eq!(got.meta.venue.as_deref(), Some("KDD"));
        assert_eq!(got.meta.year, Some(2019));
        assert_eq!(got.meta.doi.as_deref(), Some("10.1/new"));
        assert_eq!(got.meta.arxiv_id.as_deref(), Some("2001.00001"));
        assert_eq!(got.meta.dblp_key.as_deref(), Some("conf/kdd/X"));
        assert_eq!(got.rel_path, "he2016deep.pdf");
        assert_eq!(got.cite_key.as_deref(), Some("he2016deep"));
        assert_eq!(got.meta.url.as_deref(), Some("https://example.org/x"));
        assert_eq!(got.meta.source.as_deref(), Some("crossref"));
        assert_eq!(got.meta.status, PaperStatus::Resolved);
        assert_eq!(got.content_hash, "h1"); // immutable columns untouched
    }

    #[tokio::test]
    async fn all_papers_and_find_by_prefix() {
        let (_dir, pool) = temp_pool().await;
        let a = sample_paper("01890000-0000-7000-8000-0000000000a1", "ha");
        let b = sample_paper("01890000-0000-7000-8000-0000000000b2", "hb");
        insert(&pool, &a).await.unwrap();
        insert(&pool, &b).await.unwrap();

        assert_eq!(all(&pool).await.unwrap().len(), 2);

        // Unique prefix → exactly one match.
        let hit = find_by_prefix(&pool, "01890000-0000-7000-8000-0000000000a")
            .await
            .unwrap();
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].id, a.id);

        // Shared prefix → both.
        let both = find_by_prefix(&pool, "01890000").await.unwrap();
        assert_eq!(both.len(), 2);

        // LIKE wildcards in the input match only themselves.
        assert!(find_by_prefix(&pool, "0189%").await.unwrap().is_empty());
        assert!(find_by_prefix(&pool, "0189_000").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn authors_roundtrip_null_json_and_garbage() {
        let (_dir, pool) = temp_pool().await;
        // Empty -> stored NULL -> decodes empty.
        let a = sample_paper("01890000-0000-7000-8000-0000000000e5", "he");
        insert(&pool, &a).await.unwrap();
        let raw: (Option<String>,) = sqlx::query_as("SELECT authors FROM papers WHERE id = ?")
            .bind(&a.id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(raw.0, None, "empty authors must be stored as SQL NULL");
        assert!(get(&pool, &a.id)
            .await
            .unwrap()
            .unwrap()
            .meta
            .authors
            .0
            .is_empty());
        // Non-empty round-trips.
        let mut b = sample_paper("01890000-0000-7000-8000-0000000000e6", "hf");
        b.meta.authors = Authors(vec!["Kaiming He".into(), "Xiangyu Zhang".into()]);
        insert(&pool, &b).await.unwrap();
        assert_eq!(
            get(&pool, &b.id).await.unwrap().unwrap().meta.authors.0,
            vec!["Kaiming He", "Xiangyu Zhang"]
        );
        // Garbage in the column decodes to empty (legacy tolerance).
        sqlx::query("UPDATE papers SET authors = 'not json' WHERE id = ?")
            .bind(&b.id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(get(&pool, &b.id)
            .await
            .unwrap()
            .unwrap()
            .meta
            .authors
            .0
            .is_empty());
    }

    #[tokio::test]
    async fn list_papers_filters_and_sorts() {
        let (_dir, pool) = temp_pool().await;
        let mut a = sample_paper("01890000-0000-7000-8000-0000000000a1", "ha");
        a.meta.title = Some("Deep Residual Learning".into());
        a.meta.authors = Authors(vec!["Kaiming He".into()]);
        a.meta.year = Some(2016);
        a.meta.status = PaperStatus::Resolved;
        let mut b = sample_paper("01890000-0000-7000-8000-0000000000b2", "hb");
        b.meta.title = Some("Attention Is All You Need".into());
        b.meta.authors = Authors(vec!["Ashish Vaswani".into()]);
        b.meta.year = Some(2017);
        b.meta.status = PaperStatus::NeedsReview;
        insert(&pool, &a).await.unwrap();
        insert(&pool, &b).await.unwrap();

        let with_status = |status| PaperFilter {
            status: Some(status),
            ..Default::default()
        };

        // No filters → both, default sort year DESC (2017 before 2016).
        let all = list(&pool, None, Sort::default(), &PaperFilter::default())
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].meta.year, Some(2017));

        // q matches title (case-insensitive) or authors.
        let hits = list(
            &pool,
            Some("residual"),
            Sort::default(),
            &PaperFilter::default(),
        )
        .await
        .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, a.id);
        let by_author = list(
            &pool,
            Some("vaswani"),
            Sort::default(),
            &PaperFilter::default(),
        )
        .await
        .unwrap();
        assert_eq!(by_author.len(), 1);
        assert_eq!(by_author[0].id, b.id);

        // status filter.
        let resolved = list(
            &pool,
            None,
            Sort::default(),
            &with_status(PaperStatus::Resolved),
        )
        .await
        .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id, a.id);

        // q + status together (covers the AND branch).
        let combined = list(
            &pool,
            Some("attention"),
            Sort::default(),
            &with_status(PaperStatus::NeedsReview),
        )
        .await
        .unwrap();
        assert_eq!(combined.len(), 1);
        assert_eq!(combined[0].id, b.id);
        let none = list(
            &pool,
            Some("attention"),
            Sort::default(),
            &with_status(PaperStatus::Resolved),
        )
        .await
        .unwrap();
        assert!(none.is_empty());

        // year_asc sort.
        let asc = list(&pool, None, Sort::YearAsc, &PaperFilter::default())
            .await
            .unwrap();
        assert_eq!(asc[0].meta.year, Some(2016));
    }

    #[tokio::test]
    async fn by_ids_keeps_the_requested_order_and_skips_trashed() {
        let (_dir, pool) = temp_pool().await;
        let a = sample_paper("01890000-0000-7000-8000-0000000000d1", "da");
        let b = sample_paper("01890000-0000-7000-8000-0000000000d2", "db");
        let mut gone = sample_paper("01890000-0000-7000-8000-0000000000d3", "dc");
        gone.deleted_at = Some("2026-01-01T00:00:00Z".into());
        for p in [&a, &b, &gone] {
            insert(&pool, p).await.unwrap();
        }
        let ids = [
            b.id.clone(),
            gone.id.clone(),
            a.id.clone(),
            "missing".into(),
        ];
        let got = by_ids(&pool, &ids).await.unwrap();
        assert_eq!(
            got.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec![b.id.as_str(), a.id.as_str()],
        );
        assert!(by_ids(&pool, &[]).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn search_treats_like_wildcards_literally() {
        let (_dir, pool) = temp_pool().await;
        let mut a = sample_paper("01890000-0000-7000-8000-0000000000f1", "wa");
        a.meta.title = Some("100% Accurate Results".into());
        let mut b = sample_paper("01890000-0000-7000-8000-0000000000f2", "wb");
        b.meta.title = Some("1000 Accurate Results".into());
        insert(&pool, &a).await.unwrap();
        insert(&pool, &b).await.unwrap();

        // "%" must match only the literal percent title, not act as a wildcard.
        let hits = list(
            &pool,
            Some("100%"),
            Sort::default(),
            &PaperFilter::default(),
        )
        .await
        .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, a.id);
    }

    #[tokio::test]
    async fn tag_filter_matches_the_subtree_literally_and_case_insensitively() {
        let (_dir, pool) = temp_pool().await;
        let ids: Vec<String> =
            futures_util::future::join_all((0..4).map(|_| insert_test_paper(&pool))).await;
        for (id, name) in ids.iter().zip(["ML", "ml/llm", "mlx", "m_/x"]) {
            tags::add_paper(&pool, id, &tags::TagName::parse(name).unwrap())
                .await
                .unwrap();
        }
        let tagged = |tag: &str| {
            let filter = PaperFilter {
                tag: Some(tag.into()),
                ..Default::default()
            };
            let pool = pool.clone();
            async move {
                let mut got: Vec<String> = list(&pool, None, Sort::default(), &filter)
                    .await
                    .unwrap()
                    .into_iter()
                    .map(|p| p.id)
                    .collect();
                got.sort();
                got
            }
        };
        let mut ml = vec![ids[0].clone(), ids[1].clone()];
        ml.sort();
        assert_eq!(tagged("ml").await, ml);
        assert_eq!(tagged("ML/LLM").await, vec![ids[1].clone()]);
        // `_` is a literal character in a tag name, not a wildcard.
        assert_eq!(tagged("m_").await, vec![ids[3].clone()]);
        assert!(tagged("m%").await.is_empty());
    }

    #[tokio::test]
    async fn stats_counts_by_status() {
        let (_dir, pool) = temp_pool().await;
        assert_eq!(stats(&pool).await.unwrap(), (0, 0, 0));
        let mut a = sample_paper("01890000-0000-7000-8000-0000000000a1", "ha");
        a.meta.status = PaperStatus::Resolved;
        let b = sample_paper("01890000-0000-7000-8000-0000000000b2", "hb"); // needs_review
        insert(&pool, &a).await.unwrap();
        insert(&pool, &b).await.unwrap();
        assert_eq!(stats(&pool).await.unwrap(), (2, 1, 1));
    }

    #[tokio::test]
    async fn update_never_touches_trash_state() {
        let (_dir, pool) = temp_pool().await;
        let mut p = sample_paper("01890000-0000-7000-8000-0000000000d0", "hd");
        insert(&pool, &p).await.unwrap();

        // A snapshot taken before the paper was trashed (a long refresh) must
        // neither un-trash it nor overwrite its metadata.
        trash::soft_delete(&pool, &p.id).await.unwrap();
        p.meta.title = Some("Stale Rewrite".into());
        assert!(!update(&pool, &p).await.unwrap());
        let row = get(&pool, &p.id).await.unwrap().unwrap();
        assert!(row.deleted_at.is_some());
        assert_eq!(row.meta.title.as_deref(), Some("A Title"));

        // Nor can a snapshot carrying a deletion mark trash an active paper.
        trash::restore(&pool, &p.id).await.unwrap();
        p.deleted_at = Some("2026-07-07T12:00:00Z".into());
        assert!(update(&pool, &p).await.unwrap());
        assert_eq!(get(&pool, &p.id).await.unwrap().unwrap().deleted_at, None);

        assert!(!update(&pool, &sample_paper("missing", "hx")).await.unwrap());
    }

    #[tokio::test]
    async fn insert_round_trips_starred_and_name() {
        let (_dir, pool) = temp_pool().await;
        let mut p = sample_paper("01890000-0000-7000-8000-0000000000f9", "hs");
        p.starred = true;
        p.name = Some("RVSpec".into());
        insert(&pool, &p).await.unwrap();
        let got = get(&pool, &p.id).await.unwrap().unwrap();
        assert!(got.starred);
        assert_eq!(got.name.as_deref(), Some("RVSpec"));
    }

    #[tokio::test]
    async fn find_by_hash_sees_active_and_trashed() {
        let (_dir, pool) = temp_pool().await;
        assert!(find_by_hash(&pool, "abc").await.unwrap().is_none());
        let p = sample_paper("01890000-0000-7000-8000-000000000001", "abc");
        insert(&pool, &p).await.unwrap();
        assert_eq!(find_by_hash(&pool, "abc").await.unwrap().unwrap().id, p.id);
        trash::soft_delete(&pool, &p.id).await.unwrap();
        let hit = find_by_hash(&pool, "abc").await.unwrap().unwrap();
        assert!(hit.deleted_at.is_some()); // trashed rows still match
    }

    #[tokio::test]
    async fn find_by_identifier_matches_doi_or_arxiv() {
        let (_dir, pool) = temp_pool().await;
        let mut p = sample_paper("01890000-0000-7000-8000-000000000002", "h2");
        p.meta.doi = Some("10.1/x".into());
        p.meta.arxiv_id = Some("2001.00001".into());
        insert(&pool, &p).await.unwrap();

        assert_eq!(
            find_by_identifier(&pool, Some("10.1/x"), None, None)
                .await
                .unwrap()
                .unwrap()
                .id,
            p.id
        );
        assert_eq!(
            find_by_identifier(&pool, None, Some("2001.00001"), None)
                .await
                .unwrap()
                .unwrap()
                .id,
            p.id
        );
        assert!(find_by_identifier(&pool, Some("10.9/other"), None, None)
            .await
            .unwrap()
            .is_none());
        assert!(find_by_identifier(&pool, None, None, None)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn find_by_identifier_prefers_doi_over_arxiv() {
        let (_dir, pool) = temp_pool().await;
        // Row A holds only the DOI; row B holds only the arXiv id.
        let mut a = sample_paper("01890000-0000-7000-8000-000000000006", "h6");
        a.meta.doi = Some("10.1/x".into());
        insert(&pool, &a).await.unwrap();
        let mut b = sample_paper("01890000-0000-7000-8000-000000000007", "h7");
        b.meta.arxiv_id = Some("2001.00001".into());
        insert(&pool, &b).await.unwrap();

        // Both identifiers match different rows: the DOI match wins.
        let hit = find_by_identifier(&pool, Some("10.1/x"), Some("2001.00001"), None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(hit.id, a.id);

        // An unmatched DOI still falls through to the arXiv match.
        let fallback = find_by_identifier(&pool, Some("10.9/other"), Some("2001.00001"), None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fallback.id, b.id);
    }

    #[tokio::test]
    async fn unique_violation_is_detected() {
        let (_dir, pool) = temp_pool().await;
        let a = sample_paper("01890000-0000-7000-8000-000000000004", "same");
        let b = sample_paper("01890000-0000-7000-8000-000000000005", "same");
        insert(&pool, &a).await.unwrap();
        let err = insert(&pool, &b).await.unwrap_err();
        assert!(crate::db::is_unique_violation(&err));
        assert!(!crate::db::is_unique_violation(&anyhow::anyhow!(
            "something else"
        )));
    }

    #[tokio::test]
    async fn star_toggles() {
        let (_dir, pool) = temp_pool().await;
        let pid = insert_test_paper(&pool).await;
        assert!(set_starred(&pool, &pid, true).await.unwrap());
        let n: i64 = sqlx::query_scalar("SELECT starred FROM papers WHERE id = ?")
            .bind(&pid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
        set_starred(&pool, &pid, false).await.unwrap();
        let n: i64 = sqlx::query_scalar("SELECT starred FROM papers WHERE id = ?")
            .bind(&pid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn set_paper_name_sets_and_clears() {
        let (_dir, pool) = temp_pool().await;
        let pid = insert_test_paper(&pool).await;
        let rvspec = PaperName::parse("RVSpec").unwrap();
        assert!(set_name(&pool, &pid, rvspec.as_ref()).await.unwrap());
        let got = get(&pool, &pid).await.unwrap().unwrap();
        assert_eq!(got.name.as_deref(), Some("RVSpec"));
        assert!(set_name(&pool, &pid, None).await.unwrap());
        let got = get(&pool, &pid).await.unwrap().unwrap();
        assert_eq!(got.name, None);
        // Unknown id: no row updated.
        let x = PaperName::parse("X").unwrap();
        assert!(!set_name(&pool, "nope", x.as_ref()).await.unwrap());
    }

    #[tokio::test]
    async fn list_papers_sorts_by_name_nulls_last() {
        let (_dir, pool) = temp_pool().await;
        let unnamed = sample_paper("01900000-0000-7000-8000-0000000000a1", "ha");
        let mut beta = sample_paper("01900000-0000-7000-8000-0000000000b2", "hb");
        beta.name = Some("beta".into());
        let mut alpha = sample_paper("01900000-0000-7000-8000-0000000000c3", "hc");
        alpha.name = Some("Alpha".into());
        for p in [&unnamed, &beta, &alpha] {
            insert(&pool, p).await.unwrap();
        }
        let sorted = list(&pool, None, Sort::Name, &PaperFilter::default())
            .await
            .unwrap();
        // Case-insensitive alphabetical, unnamed papers last.
        assert_eq!(
            sorted.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec![alpha.id.as_str(), beta.id.as_str(), unnamed.id.as_str()],
        );
    }
}
