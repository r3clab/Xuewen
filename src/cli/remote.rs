use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Result, anyhow};
use reqwest::StatusCode;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::search::PaperLabel;
use crate::search::{IndexStatus, SearchResults};

pub(super) async fn search(
    addr: SocketAddr,
    q: &str,
    fields: Option<&str>,
    engines: Option<&str>,
) -> Result<SearchResults<PaperLabel>> {
    let mut query = vec![("q", q)];
    query.extend(fields.map(|f| ("fields", f)));
    query.extend(engines.map(|e| ("engines", e)));
    get(addr, "/api/search", &query).await
}

pub(super) async fn status(addr: SocketAddr) -> Result<IndexStatus> {
    get(addr, "/api/search/status", &[]).await
}

pub(super) enum DailyStart {
    Started,
    AlreadyRunning,
}

pub(super) async fn start_daily(addr: SocketAddr) -> Result<DailyStart> {
    let resp = client()?
        .post(format!("http://{addr}/api/daily/run"))
        .send()
        .await?;
    match resp.status() {
        StatusCode::ACCEPTED => Ok(DailyStart::Started),
        StatusCode::CONFLICT => Ok(DailyStart::AlreadyRunning),
        _ => Err(failure(resp).await),
    }
}

async fn get<T: DeserializeOwned>(
    addr: SocketAddr,
    path: &str,
    query: &[(&str, &str)],
) -> Result<T> {
    let resp = client()?
        .get(format!("http://{addr}{path}"))
        .query(query)
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(failure(resp).await);
    }
    Ok(resp.json().await?)
}

fn client() -> Result<reqwest::Client> {
    // No proxy: an HTTP_PROXY in the environment must not capture loopback.
    // The long total timeout covers the query embedding inside a search.
    Ok(reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(60))
        .build()?)
}

async fn failure(resp: reqwest::Response) -> anyhow::Error {
    #[derive(Deserialize)]
    struct ErrorBody {
        error: String,
    }
    let status = resp.status();
    match resp.json::<ErrorBody>().await {
        Ok(body) => anyhow!("{status}: {}", body.error),
        Err(e) => anyhow::Error::new(e).context(format!("{status} without an API error body")),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::app::App;
    use crate::config::DailyConfig;
    use crate::daily::DailyService;
    use crate::llm::LlmClient;
    use crate::search::embedder::Embedder;
    use crate::testutil::{serve, temp_pool, temp_vectors};

    #[tokio::test]
    async fn a_forwarded_run_starts_once_then_reports_the_one_in_flight() {
        let (dir, pool) = temp_pool().await;
        let feed = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(2)))
            .mount(&feed)
            .await;
        let (_vectors_dir, vectors) = temp_vectors(4).await;
        let daily = DailyService::for_tests(
            DailyConfig {
                categories: vec!["cs.AI".into()],
                include_cross_list: false,
                max_papers: 20,
                run_at: "09:00".into(),
                retention_days: 14,
            },
            pool.clone(),
            Embedder::for_tests("http://127.0.0.1:1/v1", "m", 4),
            vectors,
            LlmClient::new("http://127.0.0.1:1/v1", "m", None),
            &feed.uri(),
            "http://127.0.0.1:1/pdf",
        );
        let app = App::builder(pool, dir.path().to_path_buf())
            .daily(daily)
            .build();
        let tasks = app.tasks().clone();
        let addr = serve(app).await;

        assert!(matches!(
            start_daily(addr).await.unwrap(),
            DailyStart::Started
        ));
        assert!(matches!(
            start_daily(addr).await.unwrap(),
            DailyStart::AlreadyRunning
        ));
        tasks.drain().await;
    }

    #[tokio::test]
    async fn a_server_without_daily_says_so() {
        let (dir, pool) = temp_pool().await;
        let addr = serve(App::builder(pool, dir.path().to_path_buf()).build()).await;
        let err = start_daily(addr).await.err().unwrap();
        assert_eq!(
            err.to_string(),
            "503 Service Unavailable: daily papers not configured"
        );
    }

    #[tokio::test]
    async fn a_missing_endpoint_reports_the_api_error() {
        let (dir, pool) = temp_pool().await;
        let addr = serve(App::builder(pool, dir.path().to_path_buf()).build()).await;
        let err = get::<serde_json::Value>(addr, "/api/no-such", &[])
            .await
            .err()
            .unwrap();
        assert_eq!(err.to_string(), "404 Not Found: no such endpoint");
    }
}
