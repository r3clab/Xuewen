use anyhow::Result;
use sqlx::SqlitePool;
use std::collections::HashSet;

/// One recommended paper in a daily batch. Columns match `daily_papers`.
#[derive(Debug, Clone, PartialEq)]
pub struct DailyPaper {
    pub batch_date: String,
    /// 1-based, by descending score.
    pub rank: i64,
    /// Versionless arXiv id, e.g. "2507.01234".
    pub arxiv_id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub abstract_text: String,
    pub categories: Vec<String>,
    pub score: f64,
    /// `None` when TL;DR generation failed (widget falls back to abstract).
    pub tldr: Option<String>,
    pub abs_url: String,
    pub pdf_url: String,
    /// Structured five-part summary; `None` for old rows or failed generation.
    pub summary: Option<crate::summary::Summary>,
    /// First GitHub repository URL found in the paper text.
    pub code_url: Option<String>,
}

/// Raw `daily_papers` row; the JSON-in-TEXT columns decode in `into_paper`.
#[derive(sqlx::FromRow)]
struct DailyPaperRow {
    batch_date: String,
    rank: i64,
    arxiv_id: String,
    title: String,
    authors: String,
    #[sqlx(rename = "abstract")]
    abstract_text: String,
    categories: String,
    score: f64,
    tldr: Option<String>,
    abs_url: String,
    pdf_url: String,
    summary: Option<String>,
    code_url: Option<String>,
}

impl DailyPaperRow {
    fn into_paper(self) -> Result<DailyPaper> {
        // An unparsable stored summary degrades to None (old schema rows);
        // the other JSON columns are written by us and must parse.
        let summary = self
            .summary
            .as_deref()
            .and_then(|s| match serde_json::from_str(s) {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::warn!("unparsable stored summary for {}: {e}", self.arxiv_id);
                    None
                }
            });
        Ok(DailyPaper {
            batch_date: self.batch_date,
            rank: self.rank,
            arxiv_id: self.arxiv_id,
            title: self.title,
            authors: serde_json::from_str(&self.authors)?,
            abstract_text: self.abstract_text,
            categories: serde_json::from_str(&self.categories)?,
            score: self.score,
            tldr: self.tldr,
            abs_url: self.abs_url,
            pdf_url: self.pdf_url,
            summary,
            code_url: self.code_url,
        })
    }
}

/// Outcome row for one day's run. Columns match `daily_runs`.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub(crate) struct DailyRun {
    pub(crate) batch_date: String,
    /// "ok" | "empty" | "failed"
    pub(crate) status: String,
    /// Candidates after dedup, before top-N.
    pub(crate) papers_found: i64,
    /// Failure detail; also set on 'ok' runs where summaries failed.
    pub(crate) error: Option<String>,
    pub(crate) ran_at: String,
}

pub(crate) async fn record_run(pool: &SqlitePool, run: &DailyRun) -> Result<()> {
    sqlx::query(
        "INSERT INTO daily_runs (batch_date, status, papers_found, error, ran_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(batch_date) DO UPDATE SET
           status = excluded.status, papers_found = excluded.papers_found,
           error = excluded.error, ran_at = excluded.ran_at",
    )
    .bind(&run.batch_date)
    .bind(&run.status)
    .bind(run.papers_found)
    .bind(&run.error)
    .bind(&run.ran_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn get_run(pool: &SqlitePool, batch_date: &str) -> Result<Option<DailyRun>> {
    Ok(sqlx::query_as::<_, DailyRun>(
        "SELECT batch_date, status, papers_found, error, ran_at
         FROM daily_runs WHERE batch_date = ?",
    )
    .bind(batch_date)
    .fetch_optional(pool)
    .await?)
}

/// Replace `batch_date`'s papers in one transaction (re-runs overwrite).
pub async fn replace_batch(
    pool: &SqlitePool,
    batch_date: &str,
    papers: &[DailyPaper],
) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM daily_papers WHERE batch_date = ?")
        .bind(batch_date)
        .execute(&mut *tx)
        .await?;
    for p in papers {
        sqlx::query(
            "INSERT INTO daily_papers
               (batch_date, rank, arxiv_id, title, authors, abstract,
                categories, score, tldr, abs_url, pdf_url, summary, code_url)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(batch_date)
        .bind(p.rank)
        .bind(&p.arxiv_id)
        .bind(&p.title)
        .bind(serde_json::to_string(&p.authors)?)
        .bind(&p.abstract_text)
        .bind(serde_json::to_string(&p.categories)?)
        .bind(p.score)
        .bind(&p.tldr)
        .bind(&p.abs_url)
        .bind(&p.pdf_url)
        .bind(p.summary.as_ref().map(serde_json::to_string).transpose()?)
        .bind(&p.code_url)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// The newest batch that has papers, in rank order.
pub(crate) async fn latest_batch(pool: &SqlitePool) -> Result<Option<(String, Vec<DailyPaper>)>> {
    let date: Option<(String,)> =
        sqlx::query_as("SELECT batch_date FROM daily_papers ORDER BY batch_date DESC LIMIT 1")
            .fetch_optional(pool)
            .await?;
    let Some((date,)) = date else { return Ok(None) };
    let rows: Vec<DailyPaperRow> = sqlx::query_as(
        "SELECT batch_date, rank, arxiv_id, title, authors, abstract,
                categories, score, tldr, abs_url, pdf_url, summary, code_url
         FROM daily_papers WHERE batch_date = ? ORDER BY rank",
    )
    .bind(&date)
    .fetch_all(pool)
    .await?;
    let papers = rows
        .into_iter()
        .map(DailyPaperRow::into_paper)
        .collect::<Result<Vec<_>>>()?;
    Ok(Some((date, papers)))
}

/// Delete rows with `batch_date < cutoff` (YYYY-MM-DD compares correctly
/// as text) from both tables.
pub(crate) async fn prune(pool: &SqlitePool, cutoff: &str) -> Result<()> {
    sqlx::query("DELETE FROM daily_papers WHERE batch_date < ?")
        .bind(cutoff)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM daily_runs WHERE batch_date < ?")
        .bind(cutoff)
        .execute(pool)
        .await?;
    Ok(())
}

/// Every arXiv id in the library, INCLUDING trashed papers: a deleted
/// paper was a deliberate removal, so we never recommend it again. Ids are
/// normalized to versionless form so they compare equal to feed candidates.
pub(crate) async fn library_arxiv_ids(pool: &SqlitePool) -> Result<HashSet<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT arxiv_id FROM papers WHERE arxiv_id IS NOT NULL")
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(id,)| super::feed::strip_version(&id))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> sqlx::SqlitePool {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let p = crate::db::connect(&url).await.unwrap();
        std::mem::forget(dir);
        p
    }

    fn paper(date: &str, rank: i64, id: &str) -> DailyPaper {
        DailyPaper {
            batch_date: date.into(),
            rank,
            arxiv_id: id.into(),
            title: format!("Paper {id}"),
            authors: vec!["Ada Lovelace".into(), "Alan Turing".into()],
            abstract_text: "We do things.".into(),
            categories: vec!["cs.AI".into()],
            score: 0.5,
            tldr: Some("Short.".into()),
            abs_url: format!("https://arxiv.org/abs/{id}"),
            pdf_url: format!("https://arxiv.org/pdf/{id}"),
            summary: None,
            code_url: None,
        }
    }

    #[tokio::test]
    async fn record_run_upserts_by_date() {
        let pool = pool().await;
        let mut run = DailyRun {
            batch_date: "2026-07-10".into(),
            status: "failed".into(),
            papers_found: 0,
            error: Some("boom".into()),
            ran_at: "2026-07-10T09:00:00Z".into(),
        };
        record_run(&pool, &run).await.unwrap();
        run.status = "ok".into();
        run.papers_found = 5;
        run.error = None;
        record_run(&pool, &run).await.unwrap();
        let got = get_run(&pool, "2026-07-10").await.unwrap().unwrap();
        assert_eq!(got.status, "ok");
        assert_eq!(got.papers_found, 5);
        assert_eq!(got.error, None);
        assert!(get_run(&pool, "2026-07-09").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn replace_batch_and_latest_batch_roundtrip() {
        let pool = pool().await;
        replace_batch(&pool, "2026-07-09", &[paper("2026-07-09", 1, "2507.00001")])
            .await
            .unwrap();
        replace_batch(
            &pool,
            "2026-07-10",
            &[
                paper("2026-07-10", 1, "2507.00002"),
                paper("2026-07-10", 2, "2507.00003"),
            ],
        )
        .await
        .unwrap();

        let (date, papers) = latest_batch(&pool).await.unwrap().unwrap();
        assert_eq!(date, "2026-07-10");
        assert_eq!(papers.len(), 2);
        assert_eq!(papers[0].rank, 1);
        assert_eq!(papers[0].arxiv_id, "2507.00002");
        assert_eq!(papers[0].authors, vec!["Ada Lovelace", "Alan Turing"]);
        assert_eq!(papers[0].categories, vec!["cs.AI"]);

        // Re-run replaces the date's rows.
        replace_batch(&pool, "2026-07-10", &[paper("2026-07-10", 1, "2507.00009")])
            .await
            .unwrap();
        let (_, papers) = latest_batch(&pool).await.unwrap().unwrap();
        assert_eq!(papers.len(), 1);
        assert_eq!(papers[0].arxiv_id, "2507.00009");
    }

    #[tokio::test]
    async fn latest_batch_none_when_empty() {
        let pool = pool().await;
        assert!(latest_batch(&pool).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn prune_deletes_older_batches_and_runs() {
        let pool = pool().await;
        for date in ["2026-06-01", "2026-07-10"] {
            replace_batch(&pool, date, &[paper(date, 1, "x")])
                .await
                .unwrap();
            record_run(
                &pool,
                &DailyRun {
                    batch_date: date.into(),
                    status: "ok".into(),
                    papers_found: 1,
                    error: None,
                    ran_at: format!("{date}T09:00:00Z"),
                },
            )
            .await
            .unwrap();
        }
        prune(&pool, "2026-06-26").await.unwrap();
        assert!(get_run(&pool, "2026-06-01").await.unwrap().is_none());
        assert!(get_run(&pool, "2026-07-10").await.unwrap().is_some());
        let (date, _) = latest_batch(&pool).await.unwrap().unwrap();
        assert_eq!(date, "2026-07-10");
    }

    #[tokio::test]
    async fn library_arxiv_ids_includes_trashed() {
        let pool = pool().await;
        let mut p = crate::library::model::Paper {
            id: "p1".into(),
            content_hash: "h1".into(),
            rel_path: "p1.pdf".into(),
            cite_key: None,
            added_at: "2026-07-01T00:00:00Z".into(),
            deleted_at: None,
            starred: false,
            name: None,
            meta: crate::library::model::PaperMeta {
                title: Some("T".into()),
                abstract_text: None,
                authors: crate::library::model::Authors(vec![]),
                venue: None,
                year: None,
                doi: None,
                arxiv_id: Some("2401.00001".into()),
                dblp_key: None,
                url: None,
                source: None,
                status: crate::library::model::PaperStatus::Resolved,
            },
        };
        crate::library::papers::insert(&pool, &p).await.unwrap();
        p.id = "p2".into();
        p.content_hash = "h2".into();
        p.rel_path = "p2.pdf".into();
        p.meta.arxiv_id = Some("2401.00002".into());
        crate::library::papers::insert(&pool, &p).await.unwrap();
        crate::library::trash::soft_delete(&pool, "p2")
            .await
            .unwrap();

        let ids = library_arxiv_ids(&pool).await.unwrap();
        assert!(ids.contains("2401.00001"));
        assert!(ids.contains("2401.00002"), "trashed papers still dedupe");
        assert_eq!(ids.len(), 2);
    }

    #[tokio::test]
    async fn library_arxiv_ids_strips_versions() {
        let pool = pool().await;
        let p = crate::library::model::Paper {
            id: "p1".into(),
            content_hash: "h1".into(),
            rel_path: "p1.pdf".into(),
            cite_key: None,
            added_at: "2026-07-01T00:00:00Z".into(),
            deleted_at: None,
            starred: false,
            name: None,
            meta: crate::library::model::PaperMeta {
                title: Some("T".into()),
                abstract_text: None,
                authors: crate::library::model::Authors(vec![]),
                venue: None,
                year: None,
                doi: None,
                arxiv_id: Some("2401.00003v5".into()),
                dblp_key: None,
                url: None,
                source: None,
                status: crate::library::model::PaperStatus::Resolved,
            },
        };
        crate::library::papers::insert(&pool, &p).await.unwrap();

        let ids = library_arxiv_ids(&pool).await.unwrap();
        assert!(ids.contains("2401.00003"));
        assert!(!ids.contains("2401.00003v5"));
    }

    #[tokio::test]
    async fn summary_and_code_url_roundtrip() {
        let pool = pool().await;
        let mut p = paper("2026-07-10", 1, "2507.00001");
        p.summary = Some(crate::summary::Summary {
            tldr: "One line.".into(),
            problem: "Gap.".into(),
            approach: "Idea.".into(),
            results: "+4.2 on X.".into(),
            limitations: "Small data.".into(),
        });
        p.code_url = Some("https://github.com/acme/widget".into());
        replace_batch(&pool, "2026-07-10", std::slice::from_ref(&p))
            .await
            .unwrap();
        let (_, papers) = latest_batch(&pool).await.unwrap().unwrap();
        assert_eq!(papers[0].summary, p.summary);
        assert_eq!(papers[0].code_url, p.code_url);

        // Unset stays None on read (same path old NULL rows take).
        let bare = paper("2026-07-11", 1, "2507.00002");
        replace_batch(&pool, "2026-07-11", &[bare]).await.unwrap();
        let (_, papers) = latest_batch(&pool).await.unwrap().unwrap();
        assert!(papers[0].summary.is_none());
        assert!(papers[0].code_url.is_none());
    }
}
