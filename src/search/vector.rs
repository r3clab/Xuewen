use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;

const UPSERT_BATCH: usize = 64;

/// One chunk's embedding, ready for Qdrant. Chunk text stays in SQLite.
#[derive(Debug, Clone)]
pub(crate) struct ChunkPoint {
    pub(crate) paper_id: String,
    pub(crate) seq: i64,
    pub(crate) page: Option<i64>,
    pub(crate) vector: Vec<f32>,
}

#[derive(Debug, Clone)]
pub(crate) struct VecHit {
    pub(crate) paper_id: String,
    pub(crate) seq: i64,
    pub(crate) page: Option<i64>,
}

/// Restrict semantic search by chunk kind (seq 0 = title+abstract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SeqFilter {
    All,
    OnlySummary,
    OnlyBody,
}

/// Deterministic point id: UUIDv5 of "paper_id:seq" — re-upserts overwrite.
fn point_id(paper_id: &str, seq: i64) -> String {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("{paper_id}:{seq}").as_bytes(),
    )
    .to_string()
}

/// Typed wire shapes for the Qdrant responses we read. serde skips fields we
/// don't model; the ones here are required unless defaulted, so shape drift
/// fails loudly (with the response body in the error) instead of parsing as
/// zero or silently dropping hits.
#[derive(Deserialize)]
struct PointPayload {
    paper_id: String,
    seq: i64,
    /// NULL for the seq-0 title+abstract chunk.
    #[serde(default)]
    page: Option<i64>,
}

/// A collection's vector config: one unnamed vector vs. Qdrant's
/// named-vectors map — discriminated explicitly so a named-vectors
/// collection is refused outright instead of parsing its size as 0.
#[derive(Deserialize)]
#[serde(untagged)]
enum VectorsConfig {
    Params(VectorParams),
    Named(HashMap<String, VectorParams>),
}

#[derive(Deserialize)]
struct VectorParams {
    size: usize,
}

/// Truncate a response body for inclusion in an error message.
fn excerpt(body: &str) -> String {
    body.chars().take(300).collect()
}

/// Status + (truncated) body of a failed response — Qdrant returns JSON
/// diagnostics worth surfacing over a bare status code.
async fn failure(resp: reqwest::Response) -> String {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if body.trim().is_empty() {
        status.to_string()
    } else {
        format!("{status} — {}", excerpt(&body))
    }
}

/// Parse a successful response's JSON, quoting the body on a shape mismatch.
async fn parse<T: serde::de::DeserializeOwned>(what: &str, resp: reqwest::Response) -> Result<T> {
    let text = resp.text().await?;
    serde_json::from_str(&text).map_err(|e| anyhow!("qdrant {what}: {e} — {}", excerpt(&text)))
}

/// Qdrant over its REST API (the official crate would pull in the whole
/// tonic/prost gRPC stack for four calls).
pub struct QdrantStore {
    http: reqwest::Client,
    base_url: String,
    collection: String,
    dims: usize,
    /// Memoized `ensure_collection` success: the sweep validates per paper,
    /// and N identical GETs per rebuild are pointless. Failures stay
    /// un-memoized so the next call retries; `recreate_collection` resets
    /// and re-primes it. A 404 from `upsert`/`delete_paper` also resets it —
    /// the collection the memo vouches for is gone (Qdrant restarted on a
    /// fresh volume, dropped externally), and in a long-running serve nothing
    /// else would ever re-run the create cycle, wedging the vector tier
    /// until a restart.
    ensured: tokio::sync::Mutex<bool>,
}

impl QdrantStore {
    pub fn new(base_url: &str, collection: &str, dims: usize) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            base_url: base_url.trim_end_matches('/').to_string(),
            collection: collection.to_string(),
            dims,
            ensured: tokio::sync::Mutex::new(false),
        })
    }

    fn url(&self, suffix: &str) -> String {
        format!("{}/collections/{}{suffix}", self.base_url, self.collection)
    }

    /// Create the collection if missing; verify vector size if present.
    pub(crate) async fn ensure_collection(&self) -> Result<()> {
        let mut ensured = self.ensured.lock().await;
        if *ensured {
            return Ok(());
        }
        self.ensure_collection_uncached().await?;
        *ensured = true;
        Ok(())
    }

    async fn ensure_collection_uncached(&self) -> Result<()> {
        #[derive(Deserialize)]
        struct Info {
            result: InfoResult,
        }
        #[derive(Deserialize)]
        struct InfoResult {
            config: InfoConfig,
        }
        #[derive(Deserialize)]
        struct InfoConfig {
            params: InfoParams,
        }
        #[derive(Deserialize)]
        struct InfoParams {
            vectors: VectorsConfig,
        }

        let resp = self.http.get(self.url("")).send().await?;
        if resp.status().is_success() {
            let body: Info = parse("collection info", resp).await?;
            let size = match body.result.config.params.vectors {
                VectorsConfig::Params(p) => p.size,
                VectorsConfig::Named(named) => {
                    let mut names: Vec<&str> = named.keys().map(String::as_str).collect();
                    names.sort_unstable();
                    bail!(
                        "qdrant collection '{}' uses named vectors ({}), \
                         which xuewen does not support",
                        self.collection,
                        names.join(", ")
                    )
                }
            };
            if size != self.dims {
                bail!(
                    "qdrant collection '{}' has vector size {size} but config dims = {} — \
                     run: xuewen index rebuild --vectors-only",
                    self.collection,
                    self.dims
                );
            }
            return Ok(());
        }
        if resp.status().as_u16() != 404 {
            bail!("qdrant GET collection: {}", failure(resp).await);
        }
        let resp = self
            .http
            .put(self.url(""))
            .json(&json!({"vectors": {"size": self.dims, "distance": "Cosine"}}))
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("qdrant create collection: {}", failure(resp).await);
        }
        Ok(())
    }

    /// Drop and recreate the collection (vector rebuild after a dims change).
    pub(crate) async fn recreate_collection(&self) -> Result<()> {
        let mut ensured = self.ensured.lock().await;
        *ensured = false;
        let resp = self.http.delete(self.url("")).send().await?;
        if !resp.status().is_success() && resp.status().as_u16() != 404 {
            bail!("qdrant delete collection: {}", failure(resp).await);
        }
        let resp = self
            .http
            .put(self.url(""))
            .json(&json!({"vectors": {"size": self.dims, "distance": "Cosine"}}))
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("qdrant create collection: {}", failure(resp).await);
        }
        // The recreated collection is known-good for `self.dims`.
        *ensured = true;
        Ok(())
    }

    pub(crate) async fn upsert(&self, points: &[ChunkPoint]) -> Result<()> {
        for batch in points.chunks(UPSERT_BATCH) {
            let body = json!({
                "points": batch.iter().map(|p| json!({
                    "id": point_id(&p.paper_id, p.seq),
                    "vector": p.vector,
                    "payload": {"paper_id": p.paper_id, "seq": p.seq, "page": p.page},
                })).collect::<Vec<_>>()
            });
            let resp = self
                .http
                .put(format!("{}?wait=true", self.url("/points")))
                .json(&body)
                .send()
                .await?;
            if !resp.status().is_success() {
                // 404 only: the memoized collection no longer exists, so the
                // next ensure must recreate it. Transient 5xx keep the memo.
                if resp.status().as_u16() == 404 {
                    *self.ensured.lock().await = false;
                }
                bail!("qdrant upsert: {}", failure(resp).await);
            }
        }
        Ok(())
    }

    /// Vector search. `scope`, when given, restricts hits to those paper ids
    /// INSIDE Qdrant (a `match any` clause), so the top-`limit` truncation
    /// happens on the filtered set.
    pub(crate) async fn search(
        &self,
        vector: &[f32],
        limit: usize,
        filter: SeqFilter,
        scope: Option<&[String]>,
    ) -> Result<Vec<VecHit>> {
        #[derive(Deserialize)]
        struct SearchResponse {
            result: Vec<ScoredPoint>,
        }
        #[derive(Deserialize)]
        struct ScoredPoint {
            payload: PointPayload,
        }

        let mut body = json!({"vector": vector, "limit": limit, "with_payload": true});
        let mut must: Vec<serde_json::Value> = Vec::new();
        match filter {
            SeqFilter::All => {}
            SeqFilter::OnlySummary => must.push(json!({"key": "seq", "match": {"value": 0}})),
            SeqFilter::OnlyBody => must.push(json!({"key": "seq", "range": {"gte": 1}})),
        }
        if let Some(ids) = scope {
            must.push(json!({"key": "paper_id", "match": {"any": ids}}));
        }
        if !must.is_empty() {
            body["filter"] = json!({"must": must});
        }
        let resp = self
            .http
            .post(self.url("/points/search"))
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("qdrant search: {}", failure(resp).await);
        }
        let body: SearchResponse = parse("search", resp).await?;
        Ok(body
            .result
            .into_iter()
            .map(|p| VecHit {
                paper_id: p.payload.paper_id,
                seq: p.payload.seq,
                page: p.payload.page,
            })
            .collect())
    }

    /// All seq-0 (title+abstract) points as (paper_id, vector), paging
    /// through the scroll API. Feeds the daily-recommendation profile.
    pub(crate) async fn scroll_summaries(&self) -> Result<Vec<(String, Vec<f32>)>> {
        #[derive(Deserialize)]
        struct ScrollResponse {
            result: ScrollResult,
        }
        #[derive(Deserialize)]
        struct ScrollResult {
            points: Vec<ScrollPoint>,
            #[serde(default)]
            next_page_offset: Option<serde_json::Value>,
        }
        #[derive(Deserialize)]
        struct ScrollPoint {
            payload: PointPayload,
            vector: Vec<f32>,
        }

        let mut out = Vec::new();
        let mut offset: Option<serde_json::Value> = None;
        loop {
            let mut body = json!({
                "filter": {"must": [{"key": "seq", "match": {"value": 0}}]},
                "with_payload": true,
                "with_vector": true,
                "limit": 256,
            });
            if let Some(o) = &offset {
                body["offset"] = o.clone();
            }
            let resp = self
                .http
                .post(self.url("/points/scroll"))
                .json(&body)
                .send()
                .await?;
            if !resp.status().is_success() {
                bail!("qdrant scroll: {}", failure(resp).await);
            }
            let body: ScrollResponse = parse("scroll", resp).await?;
            for p in body.result.points {
                out.push((p.payload.paper_id, p.vector));
            }
            offset = body.result.next_page_offset;
            if offset.is_none() {
                break;
            }
        }
        Ok(out)
    }

    pub(crate) async fn delete_paper(&self, paper_id: &str) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}?wait=true", self.url("/points/delete")))
            .json(&json!({"filter": {"must": [{"key": "paper_id", "match": {"value": paper_id}}]}}))
            .send()
            .await?;
        if !resp.status().is_success() {
            if resp.status().as_u16() == 404 {
                *self.ensured.lock().await = false;
            }
            bail!("qdrant delete: {}", failure(resp).await);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn store(server: &MockServer) -> QdrantStore {
        QdrantStore::new(&server.uri(), "xuewen", 4).unwrap()
    }

    #[test]
    fn point_ids_are_deterministic_uuids() {
        let a = point_id("p1", 0);
        assert_eq!(a, point_id("p1", 0));
        assert_ne!(a, point_id("p1", 1));
        assert_ne!(a, point_id("p2", 0));
        assert!(uuid::Uuid::parse_str(&a).is_ok());
    }

    #[tokio::test]
    async fn ensure_creates_missing_collection() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/collections/xuewen"))
            .and(body_partial_json(
                json!({"vectors": {"size": 4, "distance": "Cosine"}}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": true})))
            .expect(1)
            .mount(&server)
            .await;
        store(&server).ensure_collection().await.unwrap();
    }

    #[tokio::test]
    async fn ensure_rejects_dims_mismatch() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {"config": {"params": {"vectors": {"size": 8, "distance": "Cosine"}}}}
            })))
            .mount(&server)
            .await;
        let err = store(&server)
            .ensure_collection()
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("rebuild --vectors-only"), "got: {err}");
    }

    #[tokio::test]
    async fn upsert_sends_points_with_payload() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/collections/xuewen/points"))
            .and(body_partial_json(
                json!({"points": [{"payload": {"paper_id": "p1", "seq": 0}}]}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {}})))
            .expect(1)
            .mount(&server)
            .await;
        let pts = vec![ChunkPoint {
            paper_id: "p1".into(),
            seq: 0,
            page: None,
            vector: vec![0.1; 4],
        }];
        store(&server).upsert(&pts).await.unwrap();
    }

    #[tokio::test]
    async fn search_parses_hits_and_applies_seq_filter() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/search"))
            .and(body_partial_json(
                json!({"filter": {"must": [{"key": "seq", "range": {"gte": 1}}]}}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": [
                    {"id": "x", "score": 0.9, "payload": {"paper_id": "p1", "seq": 3, "page": 7}},
                    {"id": "y", "score": 0.5, "payload": {"paper_id": "p2", "seq": 1, "page": 2}}
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let hits = store(&server)
            .search(&[0.1; 4], 10, SeqFilter::OnlyBody, None)
            .await
            .unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].paper_id, "p1");
        assert_eq!(hits[0].seq, 3);
        assert_eq!(hits[0].page, Some(7));
    }

    #[tokio::test]
    async fn search_scope_becomes_a_paper_id_filter_clause() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/search"))
            .and(body_partial_json(json!({"filter": {"must": [
                {"key": "seq", "range": {"gte": 1}},
                {"key": "paper_id", "match": {"any": ["p1", "p2"]}}
            ]}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": []})))
            .expect(1)
            .mount(&server)
            .await;
        let scope = vec!["p1".to_string(), "p2".to_string()];
        store(&server)
            .search(&[0.1; 4], 10, SeqFilter::OnlyBody, Some(&scope))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn search_error_includes_qdrant_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/search"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(json!({"status": {"error": "wrong vector size"}})),
            )
            .mount(&server)
            .await;
        let err = store(&server)
            .search(&[0.1; 4], 10, SeqFilter::All, None)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("wrong vector size"), "got: {err}");
    }

    #[tokio::test]
    async fn named_vectors_collection_is_rejected_explicitly() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {"config": {"params": {"vectors": {
                    "custom": {"size": 4, "distance": "Cosine"}
                }}}}
            })))
            .mount(&server)
            .await;
        let err = store(&server)
            .ensure_collection()
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("named vectors"), "got: {err}");
    }

    #[tokio::test]
    async fn ensure_success_is_memoized_and_recreate_reprimes_it() {
        let server = MockServer::start().await;
        // Exactly one GET despite three ensure calls: success is memoized,
        // and a successful recreate re-primes the cache (the collection it
        // just created is known-good).
        Mock::given(method("GET"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {"config": {"params": {"vectors": {"size": 4, "distance": "Cosine"}}}}
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": true})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": true})))
            .expect(1)
            .mount(&server)
            .await;

        let s = store(&server);
        s.ensure_collection().await.unwrap();
        s.ensure_collection().await.unwrap();
        s.recreate_collection().await.unwrap();
        s.ensure_collection().await.unwrap();
    }

    #[tokio::test]
    async fn ensure_failure_is_not_memoized() {
        let server = MockServer::start().await;
        // First GET fails; the next ensure must retry rather than trust a
        // cached failure.
        Mock::given(method("GET"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(500))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {"config": {"params": {"vectors": {"size": 4, "distance": "Cosine"}}}}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let s = store(&server);
        assert!(s.ensure_collection().await.is_err());
        s.ensure_collection().await.unwrap();
    }

    #[tokio::test]
    async fn upsert_404_resets_the_ensure_memo_and_the_tier_heals() {
        let server = MockServer::start().await;
        // First ensure sees the collection; it then vanishes (Qdrant restarted
        // on a fresh volume), the upsert 404s, and the next ensure must run
        // the full GET(404)+PUT create cycle instead of trusting the memo.
        Mock::given(method("GET"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {"config": {"params": {"vectors": {"size": 4, "distance": "Cosine"}}}}
            })))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/collections/xuewen"))
            .and(body_partial_json(
                json!({"vectors": {"size": 4, "distance": "Cosine"}}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": true})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/collections/xuewen/points"))
            .respond_with(ResponseTemplate::new(404))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/collections/xuewen/points"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {}})))
            .expect(1)
            .mount(&server)
            .await;

        let s = store(&server);
        let pts = vec![ChunkPoint {
            paper_id: "p1".into(),
            seq: 0,
            page: None,
            vector: vec![0.1; 4],
        }];
        s.ensure_collection().await.unwrap();
        assert!(s.upsert(&pts).await.is_err());
        s.ensure_collection().await.unwrap();
        s.upsert(&pts).await.unwrap();
    }

    #[tokio::test]
    async fn delete_404_resets_the_ensure_memo() {
        let server = MockServer::start().await;
        // Two GETs despite the memo: the delete's 404 un-memoizes.
        Mock::given(method("GET"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {"config": {"params": {"vectors": {"size": 4, "distance": "Cosine"}}}}
            })))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/delete"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;

        let s = store(&server);
        s.ensure_collection().await.unwrap();
        assert!(s.delete_paper("p1").await.is_err());
        s.ensure_collection().await.unwrap();
    }

    #[tokio::test]
    async fn upsert_5xx_keeps_the_ensure_memo() {
        let server = MockServer::start().await;
        // A transient server error is not a vanished collection: exactly one
        // GET, even after the failed upsert.
        Mock::given(method("GET"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {"config": {"params": {"vectors": {"size": 4, "distance": "Cosine"}}}}
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/collections/xuewen/points"))
            .respond_with(ResponseTemplate::new(500))
            .expect(1)
            .mount(&server)
            .await;

        let s = store(&server);
        let pts = vec![ChunkPoint {
            paper_id: "p1".into(),
            seq: 0,
            page: None,
            vector: vec![0.1; 4],
        }];
        s.ensure_collection().await.unwrap();
        assert!(s.upsert(&pts).await.is_err());
        s.ensure_collection().await.unwrap();
    }

    #[tokio::test]
    async fn delete_paper_filters_on_payload() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/delete"))
            .and(body_partial_json(
                json!({"filter": {"must": [{"key": "paper_id", "match": {"value": "p1"}}]}}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {}})))
            .expect(1)
            .mount(&server)
            .await;
        store(&server).delete_paper("p1").await.unwrap();
    }

    #[tokio::test]
    async fn recreate_collection_tolerates_missing_then_creates() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/collections/xuewen"))
            .and(body_partial_json(
                json!({"vectors": {"size": 4, "distance": "Cosine"}}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": true})))
            .expect(1)
            .mount(&server)
            .await;
        store(&server).recreate_collection().await.unwrap();
    }

    #[tokio::test]
    async fn ensure_is_noop_when_size_matches() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {"config": {"params": {"vectors": {"size": 4, "distance": "Cosine"}}}}
            })))
            .expect(1)
            .mount(&server)
            .await;
        store(&server).ensure_collection().await.unwrap();
    }

    #[tokio::test]
    async fn upsert_batches_at_64_points() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/collections/xuewen/points"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {}})))
            .expect(2)
            .mount(&server)
            .await;
        let points: Vec<ChunkPoint> = (0..65)
            .map(|i| ChunkPoint {
                paper_id: "p1".into(),
                seq: i,
                page: None,
                vector: vec![0.1; 4],
            })
            .collect();
        store(&server).upsert(&points).await.unwrap();
    }

    #[tokio::test]
    async fn scroll_summaries_pages_until_offset_is_null() {
        let server = MockServer::start().await;
        // Page 2 (has "offset" in the body) — mount FIRST so it wins when it matches.
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/scroll"))
            .and(body_partial_json(json!({"offset": "cursor-1"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {
                    "points": [
                        {"id": "b", "payload": {"paper_id": "p2", "seq": 0}, "vector": [0.0, 1.0, 0.0, 0.0]}
                    ],
                    "next_page_offset": null
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        // Page 1: filters seq=0, requests vectors.
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/scroll"))
            .and(body_partial_json(json!({
                "filter": {"must": [{"key": "seq", "match": {"value": 0}}]},
                "with_vector": true
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {
                    "points": [
                        {"id": "a", "payload": {"paper_id": "p1", "seq": 0}, "vector": [1.0, 0.0, 0.0, 0.0]}
                    ],
                    "next_page_offset": "cursor-1"
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let s = store(&server);
        let out = s.scroll_summaries().await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, "p1");
        assert_eq!(out[0].1, vec![1.0, 0.0, 0.0, 0.0]);
        assert_eq!(out[1].0, "p2");
    }
}
