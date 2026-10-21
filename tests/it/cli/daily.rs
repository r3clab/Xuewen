use xuewen::daily::store::{self, DailyPaper};
use xuewen::search::vector::{OpenMode, VectorStore};

use super::{fails, library, library_with, ok};

const CONFIGURED: &str = "[daily]\ncategories = [\"cs.AI\"]\n\n[ai]\nbase_url = \"http://127.0.0.1:1/v1\"\napi_key = \"k\"\nmodel = \"m\"\n\n[ai.embedding]\nmodel = \"m1\"\ndims = 4\n\n[ai.daily]\n";

async fn record_run(pool: &sqlx::SqlitePool, date: &str, status: &str) {
    sqlx::query(
        "INSERT INTO daily_runs (batch_date, status, papers_found, error, ran_at) VALUES (?, ?, 3, NULL, ?)",
    )
    .bind(date)
    .bind(status)
    .bind(format!("{date}T09:00:00Z"))
    .execute(pool)
    .await
    .unwrap();
}

fn daily_paper(date: &str, rank: i64, id: &str, tldr: Option<&str>) -> DailyPaper {
    DailyPaper {
        batch_date: date.into(),
        rank,
        arxiv_id: id.into(),
        title: format!("Paper {id}"),
        authors: vec!["Ada".into()],
        abstract_text: "An abstract.".into(),
        categories: vec!["cs.AI".into()],
        score: 0.9,
        tldr: tldr.map(String::from),
        abs_url: format!("https://arxiv.org/abs/{id}"),
        pdf_url: format!("https://arxiv.org/pdf/{id}"),
        summary: None,
        code_url: None,
    }
}

#[tokio::test]
async fn show_with_no_runs_says_how_to_start_one() {
    let (_pool, config) = library().await;
    assert_eq!(
        ok(&config, &["daily", "show"]),
        "no daily runs yet — run: xuewen daily run\n"
    );
}

#[tokio::test]
async fn show_prints_the_latest_run_then_the_latest_papers() {
    let (pool, config) = library().await;
    store::replace_batch(
        &pool,
        "2026-07-09",
        &[
            daily_paper("2026-07-09", 1, "2507.1", Some("Short.")),
            daily_paper("2026-07-09", 2, "2507.2", None),
        ],
    )
    .await
    .unwrap();
    record_run(&pool, "2026-07-10", "empty").await;

    assert_eq!(
        ok(&config, &["daily", "show"]),
        "2026-07-10: empty (3 candidates), ran 2026-07-10T09:00:00Z\n\
         latest papers, from 2026-07-09:\n \
         1. 2507.1  Paper 2507.1  (0.900)\n      Short.\n \
         2. 2507.2  Paper 2507.2  (0.900)\n"
    );
}

#[tokio::test]
async fn run_without_daily_config_says_why() {
    let (_pool, config) = library().await;
    let err = fails(&config, &["daily", "run"]);
    assert!(
        err.contains("daily papers are off: [daily] is not configured"),
        "got: {err}"
    );
}

#[tokio::test]
async fn run_leaves_a_finished_day_alone_unless_forced() {
    let (pool, config) = library().await;
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    record_run(&pool, &today, "ok").await;

    let out = ok(&config, &["daily", "run"]);
    assert!(
        out.contains("today's run already finished — pass --force to run it again"),
        "got: {out}"
    );

    let err = fails(&config, &["daily", "run", "--force"]);
    assert!(
        err.contains("[daily] is not configured"),
        "--force goes past the check: {err}"
    );
}

#[tokio::test]
async fn run_beside_a_process_holding_the_vector_store_names_it() {
    let (_pool, config) = library_with(CONFIGURED).await;
    let vectors = config.parent().unwrap().join("vectors");
    let _holder = VectorStore::open(&vectors, 4, OpenMode::LoadOrCreate)
        .await
        .unwrap();

    let err = fails(&config, &["daily", "run"]);
    assert!(
        err.contains("daily papers are off: the vector store is unavailable (vector store")
            && err.contains("is in use by another xuewen process"),
        "got: {err}"
    );
}
