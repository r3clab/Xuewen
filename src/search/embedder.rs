use crate::http::{HttpClient, RetryPolicy};
use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use std::time::Duration;

const BATCH: usize = 64;

/// Client for an OpenAI-compatible `/embeddings` endpoint.
pub struct Embedder {
    /// Shared retrying transport (`crate::http`) — same retry/backoff
    /// implementation as every other HTTP caller in the crate.
    http: HttpClient,
    base_url: String,
    model: String,
    dims: usize,
    api_key: Option<String>,
}

fn embedding_http() -> HttpClient {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .expect("building embedding HTTP client");
    HttpClient::new(client, RetryPolicy::llm())
}

impl Embedder {
    /// Build from a resolved endpoint + dims. `None` when no key resolves.
    pub(crate) fn from_resolved(
        r: &crate::config::Resolved,
        model: &str,
        dims: usize,
    ) -> Option<Self> {
        let Some(key) = r.api_key.clone() else {
            tracing::warn!("[ai.embedding] configured but no API key — semantic search disabled");
            return None;
        };
        Some(Self {
            http: embedding_http(),
            base_url: r.base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            dims,
            api_key: Some(key),
        })
    }

    /// Keyless client pointed at a mock server. Test support only.
    pub fn for_tests(base_url: &str, model: &str, dims: usize) -> Self {
        Self {
            http: embedding_http(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            dims,
            api_key: None,
        }
    }

    pub(crate) fn model(&self) -> &str {
        &self.model
    }

    pub(crate) async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for batch in texts.chunks(BATCH) {
            out.extend(self.embed_batch(batch).await?);
        }
        Ok(out)
    }

    async fn embed_batch(&self, batch: &[String]) -> Result<Vec<Vec<f32>>> {
        #[derive(Deserialize)]
        struct Item {
            index: usize,
            embedding: Vec<f32>,
        }
        #[derive(Deserialize)]
        struct Body {
            data: Vec<Item>,
        }

        let mut req = self
            .http
            .post(&format!("{}/embeddings", self.base_url))
            .json(&serde_json::json!({ "model": self.model, "input": batch }));
        if let Some(k) = &self.api_key {
            req = req.bearer_auth(k);
        }
        let text = self
            .http
            .send_text(req)
            .await
            .map_err(|e| anyhow!("embedding API: {e}"))?;
        let mut body: Body = serde_json::from_str(&text)?;
        if body.data.len() != batch.len() {
            bail!(
                "embedding API returned {} vectors for {} inputs",
                body.data.len(),
                batch.len()
            );
        }
        body.data.sort_by_key(|d| d.index);
        for d in &body.data {
            if d.embedding.len() != self.dims {
                bail!(
                    "embedding dims mismatch: API returned {}, config says {} — fix [ai.embedding].dims",
                    d.embedding.len(),
                    self.dims
                );
            }
        }
        Ok(body.data.into_iter().map(|d| d.embedding).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    fn embedding_response(n: usize, dims: usize) -> serde_json::Value {
        let data: Vec<_> = (0..n)
            .map(|i| json!({"index": i, "embedding": vec![0.1f32; dims]}))
            .collect();
        json!({"data": data})
    }

    #[tokio::test]
    async fn embeds_with_bearer_auth_and_model() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .and(header("authorization", "Bearer sk-test"))
            .and(body_partial_json(
                json!({"model": "text-embedding-3-small"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(embedding_response(2, 4)))
            .expect(1)
            .mount(&server)
            .await;

        let r = crate::config::Resolved {
            base_url: format!("{}/v1", server.uri()),
            api_key: Some("sk-test".into()),
            api_key_env: "OPENAI_API_KEY".into(),
            model: Some("text-embedding-3-small".into()),
            reasoning_effort: None,
        };
        let e = Embedder::from_resolved(&r, "text-embedding-3-small", 4).unwrap();
        let out = e.embed(&["a".into(), "b".into()]).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].len(), 4);
    }

    #[tokio::test]
    async fn batches_requests_of_64() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(move |req: &Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                let n = body["input"].as_array().unwrap().len();
                assert!(n <= 64, "batch too large: {n}");
                ResponseTemplate::new(200).set_body_json(embedding_response(n, 4))
            })
            .expect(2) // 100 texts -> 64 + 36
            .mount(&server)
            .await;

        let e = Embedder::for_tests(&format!("{}/v1", server.uri()), "m", 4);
        let texts: Vec<String> = (0..100).map(|i| format!("t{i}")).collect();
        let out = e.embed(&texts).await.unwrap();
        assert_eq!(out.len(), 100);
    }

    #[tokio::test]
    async fn retries_on_429_then_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(embedding_response(1, 4)))
            .expect(1)
            .mount(&server)
            .await;

        let e = Embedder::for_tests(&format!("{}/v1", server.uri()), "m", 4);
        let out = e.embed(&["a".into()]).await.unwrap();
        assert_eq!(out.len(), 1);
    }

    #[tokio::test]
    async fn wrong_dims_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(embedding_response(1, 3)))
            .mount(&server)
            .await;
        let e = Embedder::for_tests(&format!("{}/v1", server.uri()), "m", 4);
        let err = e.embed(&["a".into()]).await.unwrap_err().to_string();
        assert!(err.contains("dims"), "got: {err}");
    }

    #[test]
    fn from_resolved_without_key_is_none() {
        let r = crate::config::Resolved {
            base_url: "https://api.openai.com/v1".into(),
            api_key: None,
            api_key_env: "OPENAI_API_KEY".into(),
            model: Some("m".into()),
            reasoning_effort: None,
        };
        assert!(Embedder::from_resolved(&r, "m", 4).is_none());
    }
}
