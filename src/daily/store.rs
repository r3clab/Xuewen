use anyhow::Result;
use sqlx::SqlitePool;
use std::collections::HashSet;

/// One recommended paper in a daily batch. Columns match `daily_papers`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct DailyPaper {
    #[serde(skip)]
    pub batch_date: String,
    /// 1-based, by descending score.
    pub rank: i64,
    /// Versionless arXiv id, e.g. "2507.01234".
    pub arxiv_id: String,
    pub title: String,
    pub authors: Vec<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: String,
    pub categories: Vec<String>,
    pub score: f64,
    pub abs_url: String,
    pub pdf_url: String,
    /// Structured five-part summary; `None` for old rows or failed generation.
    pub summary: Option<crate::summary::Summary>,
    /// First GitHub repository URL found in the paper text.
    pub code_url: Option<String>,
}

impl DailyPaper {
    /// `None` when summary generation failed (the widget falls back to the
    /// abstract).
    pub fn tldr(&self) -> Option<&str> {
        self.summary.as_ref().map(|s| s.tldr.as_str())
    }
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
            abs_url: self.abs_url,
            pdf_url: self.pdf_url,
            summary,
            code_url: self.code_url,
        })
    }
}

/// `daily_runs.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub(crate) enum DailyStatus {
    Ok,
    /// The feed had no new candidates.
    Empty,
    Failed,
}

impl DailyStatus {
    /// Whether the day is done; a failed run is retried.
    pub(crate) fn settled(self) -> bool {
        self != Self::Failed
    }
}

impl std::fmt::Display for DailyStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Ok => "ok",
            Self::Empty => "empty",
            Self::Failed => "failed",
        })
    }
}

/// Outcome row for one day's run. Columns match `daily_runs`.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub(crate) struct DailyRun {
    pub(crate) batch_date: String,
    pub(crate) status: DailyStatus,
    /// Candidates after dedup, before top-N.
    pub(crate) papers_found: i64,
    /// Failure detail; also set on 'ok' runs where summaries failed.
    pub(crate) error: Option<String>,
    pub(crate) ran_at: String,
}

impl std::fmt::Display for DailyRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {} ({} candidates)",
            self.batch_date, self.status, self.papers_found
        )
    }
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
    .bind(run.status)
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

/// The most recent day's run, whatever its outcome.
pub(crate) async fn latest_run(pool: &SqlitePool) -> Result<Option<DailyRun>> {
    Ok(sqlx::query_as::<_, DailyRun>(
        "SELECT batch_date, status, papers_found, error, ran_at
         FROM daily_runs ORDER BY batch_date DESC LIMIT 1",
    )
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
                categories, score, abs_url, pdf_url, summary, code_url)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(batch_date)
        .bind(p.rank)
        .bind(&p.arxiv_id)
        .bind(&p.title)
        .bind(serde_json::to_string(&p.authors)?)
        .bind(&p.abstract_text)
        .bind(serde_json::to_string(&p.categories)?)
        .bind(p.score)
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
                categories, score, abs_url, pdf_url, summary, code_url
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
        .map(|(id,)| crate::library::model::canonical_arxiv(&id))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::model::{ARXIV_ABS_BASE, ARXIV_PDF_BASE};

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
            abs_url: format!("{ARXIV_ABS_BASE}/{id}"),
            pdf_url: format!("{ARXIV_PDF_BASE}/{id}"),
            summary: None,
            code_url: None,
        }
    }

    #[tokio::test]
    async fn record_run_upserts_by_date() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        let mut run = DailyRun {
            batch_date: "2026-07-10".into(),
            status: DailyStatus::Failed,
            papers_found: 0,
            error: Some("boom".into()),
            ran_at: "2026-07-10T09:00:00Z".into(),
        };
        record_run(&pool, &run).await.unwrap();
        let stored: String = sqlx::query_scalar("SELECT status FROM daily_runs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(stored, "failed");
        run.status = DailyStatus::Ok;
        run.papers_found = 5;
        run.error = None;
        record_run(&pool, &run).await.unwrap();
        let got = get_run(&pool, "2026-07-10").await.unwrap().unwrap();
        assert_eq!(got.status, DailyStatus::Ok);
        assert_eq!(got.papers_found, 5);
        assert_eq!(got.error, None);
        assert!(get_run(&pool, "2026-07-09").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn latest_run_is_the_newest_day() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        assert!(latest_run(&pool).await.unwrap().is_none());
        for date in ["2026-07-11", "2026-07-09"] {
            let run = DailyRun {
                batch_date: date.into(),
                status: DailyStatus::Empty,
                papers_found: 0,
                error: None,
                ran_at: format!("{date}T09:00:00Z"),
            };
            record_run(&pool, &run).await.unwrap();
        }
        let got = latest_run(&pool).await.unwrap().unwrap();
        assert_eq!(got.batch_date, "2026-07-11");
    }

    #[tokio::test]
    async fn replace_batch_and_latest_batch_roundtrip() {
        let (_db, pool) = crate::testutil::temp_pool().await;
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
        let (_db, pool) = crate::testutil::temp_pool().await;
        assert!(latest_batch(&pool).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn prune_deletes_older_batches_and_runs() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        for date in ["2026-06-01", "2026-07-10"] {
            replace_batch(&pool, date, &[paper(date, 1, "x")])
                .await
                .unwrap();
            record_run(
                &pool,
                &DailyRun {
                    batch_date: date.into(),
                    status: DailyStatus::Ok,
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
        let (_db, pool) = crate::testutil::temp_pool().await;
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
        let (_db, pool) = crate::testutil::temp_pool().await;
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
        let (_db, pool) = crate::testutil::temp_pool().await;
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

    #[tokio::test]
    async fn dropping_the_tldr_copy_folds_it_into_a_summary_lacking_one() {
        let (_dir, pool) = crate::db::pool_before_migration("drop daily tldr").await;
        let full =
            r#"{"tldr":"Kept.","problem":"P","approach":"A","results":"R","limitations":"L"}"#;
        let tldr_less = r#"{"problem":"P","approach":"A","results":"R","limitations":"L"}"#;
        for (rank, summary) in [(1, Some(tldr_less)), (2, Some(full)), (3, None)] {
            sqlx::query(
                "INSERT INTO daily_papers (batch_date, rank, arxiv_id, title, authors, abstract, \
                 categories, score, tldr, abs_url, pdf_url, summary) \
                 VALUES ('2026-07-10', ?, ?, 't', '[]', 'a', '[]', 0.5, 'Copied.', 'u', 'u', ?)",
            )
            .bind(rank)
            .bind(format!("2507.0000{rank}"))
            .bind(summary)
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::migrate!().run(&pool).await.unwrap();

        let (_, papers) = latest_batch(&pool).await.unwrap().unwrap();
        let tldrs: Vec<Option<&str>> = papers.iter().map(DailyPaper::tldr).collect();
        assert_eq!(tldrs, [Some("Copied."), Some("Kept."), None]);
    }
}
