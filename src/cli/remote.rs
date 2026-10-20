use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{bail, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::search::IndexStatus;

/// The slice of `GET /api/search` the CLI prints. A local struct rather than
/// the web types, which are serialize-only.
#[derive(Deserialize)]
pub(super) struct SearchReply {
    pub(super) semantic: Availability,
    pub(super) results: Vec<Hit>,
}

#[derive(Deserialize)]
pub(super) struct Availability {
    pub(super) reason: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct Hit {
    pub(super) paper: HitPaper,
    #[serde(rename = "match")]
    pub(super) matched: HitMatch,
}

#[derive(Deserialize)]
pub(super) struct HitPaper {
    pub(super) id: String,
    pub(super) cite_key: Option<String>,
    pub(super) title: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct HitMatch {
    pub(super) field: String,
    pub(super) snippet: String,
    pub(super) page: Option<i64>,
}

pub(super) async fn search(
    addr: SocketAddr,
    q: &str,
    fields: Option<&str>,
    engines: Option<&str>,
) -> Result<SearchReply> {
    let mut query = vec![("q", q)];
    query.extend(fields.map(|f| ("fields", f)));
    query.extend(engines.map(|e| ("engines", e)));
    get(addr, "/api/search", &query).await
}

pub(super) async fn status(addr: SocketAddr) -> Result<IndexStatus> {
    get(addr, "/api/search/status", &[]).await
}

async fn get<T: DeserializeOwned>(
    addr: SocketAddr,
    path: &str,
    query: &[(&str, &str)],
) -> Result<T> {
    // No proxy: an HTTP_PROXY in the environment must not capture loopback.
    // The long total timeout covers the query embedding inside a search.
    let client = reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(60))
        .build()?;
    let resp = client
        .get(format!("http://{addr}{path}"))
        .query(query)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("{status}: {}", body.trim());
    }
    Ok(resp.json().await?)
}
