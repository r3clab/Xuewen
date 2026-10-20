pub(crate) mod heuristic;
mod store;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;

use crate::config::Config;

/// One bibliography entry parsed to fields. Field names are the JSON wire
/// format shared with the frontend.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub(crate) struct StructuredReference {
    // `null_as_empty`: the model sometimes emits `"authors": null` for an
    // entry it half-parsed; that must not fail the whole batch.
    #[serde(default, deserialize_with = "null_as_empty")]
    authors: Vec<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    venue: Option<String>,
    #[serde(default)]
    year: Option<i64>,
    #[serde(default)]
    doi: Option<String>,
    #[serde(default)]
    arxiv_id: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

fn null_as_empty<'de, D>(d: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: Option<Vec<String>> = Option::deserialize(d)?;
    Ok(v.unwrap_or_default())
}

pub struct CitationsService {
    pool: SqlitePool,
    /// LLM fallback for entries the heuristic can't parse; `None` (no
    /// `[ai.citations]`) leaves those entries null.
    llm: Option<crate::llm::LlmClient>,
    /// Per-paper single-flight. The reader POSTs on every paper open and the
    /// LLM leg takes seconds with the cache only written at the end, so a
    /// reopened tab (or a second window) would otherwise pay the full token
    /// cost again; a second caller awaits the first and then hits the cache
    /// it just wrote. An entry is dropped when its last flight lands, so the
    /// map stays O(open papers).
    in_flight: std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

/// Row-level provenance stored in the cache's `model` column: `heuristic-v1`
/// when heuristics alone wrote the row, `heuristic-v1+<model>` when an LLM
/// filled leftovers. The exact-`heuristic-v1` spelling gates the "retry
/// nulls once an LLM appears" upgrade — `+<model>` means the LLM already
/// declined those nulls, so they are not retried on every open.
const HEURISTIC_VERSION: &str = "heuristic-v1";
/// Per-entry provenance (`src` in the cached JSON) for a heuristic parse;
/// LLM entries carry `llm:<model>`. Bump the generation here AND in
/// `HEURISTIC_VERSION` after heuristic improvements: entries from an older
/// `h*` generation are re-parsed and overwritten — a wrong `Some` included —
/// while LLM and origin-unknown entries are never degraded.
const HEURISTIC_SRC: &str = "h1";

/// Max bibliography entries per LLM call. Large batches make the model drop
/// or merge entries (observed live: 111 in → 69 out; 68 → 52), which trips
/// the exact-length validation on every retry. Small chunks count reliably;
/// results are concatenated in order.
const CHUNK_SIZE: usize = 25;

const SYSTEM: &str = "You convert bibliography entries from research papers into structured JSON. \
Output ONLY a JSON array — no prose, no markdown fences.";

/// One cached entry as stored in the `parsed` column: the wire fields plus
/// per-entry provenance. `src` is `None` on rows written before per-entry
/// provenance existed (and is kept off such entries when a row is
/// rewritten): entries in one row have mixed origins, and only the entry
/// itself can say whether it is safe to overwrite. The wire type stays
/// `StructuredReference` — `src` never leaves the cache.
#[derive(Serialize, Deserialize)]
struct CachedReference {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    src: Option<String>,
    #[serde(flatten)]
    reference: StructuredReference,
}

/// Whether a cached entry is one a `HEURISTIC_SRC` bump overwrites: a
/// heuristic parse (`h*`) from a generation other than the current one.
/// LLM (`llm:*`) and origin-unknown (`None`) entries are never overwritten —
/// degrading a possible LLM parse is worse than serving a stale heuristic
/// one.
fn stale_heuristic(src: Option<&str>) -> bool {
    src.is_some_and(|s| s.starts_with('h') && s != HEURISTIC_SRC)
}

impl CitationsService {
    /// Always available — the heuristic needs no config. `[ai.citations]`
    /// adds the LLM fallback; absent (silently) or present but missing a
    /// model/API key (warns) → heuristics only.
    pub fn from_config(pool: SqlitePool, cfg: &Config) -> Arc<Self> {
        let llm = cfg.ai.citations.as_ref().and_then(|use_| {
            let client = crate::llm::LlmClient::from_resolved(&cfg.ai.resolve(use_));
            if client.is_none() {
                tracing::warn!("[ai.citations] has no model or API key — LLM fallback disabled");
            }
            client
        });
        if llm.is_none() {
            tracing::info!("citation parsing: heuristics only (no [ai.citations] LLM fallback)");
        }
        Arc::new(Self::new(pool, llm))
    }

    /// Heuristics-only service (no LLM). Test-router support; production
    /// goes through `from_config`, which handles the no-LLM case itself.
    pub(crate) fn heuristic_only(pool: SqlitePool) -> Arc<Self> {
        Arc::new(Self::new(pool, None))
    }

    /// Keyless client pointed at a mock server. Test support only.
    pub fn for_tests(pool: SqlitePool, base_url: &str, model: &str) -> Arc<Self> {
        Arc::new(Self::new(
            pool,
            Some(crate::llm::LlmClient::new(base_url, model, None)),
        ))
    }

    fn new(pool: SqlitePool, llm: Option<crate::llm::LlmClient>) -> Self {
        Self {
            pool,
            llm,
            in_flight: Default::default(),
        }
    }

    /// Parse `refs` for `paper_id`: exact-input cache hit, else heuristics
    /// (style vote seeded by `venue`), then the LLM — in `CHUNK_SIZE`
    /// batches — for only the entries heuristics couldn't parse. The result
    /// is index-aligned with `refs`. Concurrent calls for one paper
    /// single-flight (see `in_flight`).
    pub(crate) async fn parse(
        &self,
        paper_id: &str,
        refs: &[String],
        venue: Option<&str>,
    ) -> Result<Vec<Option<StructuredReference>>> {
        let gate = self
            .in_flight
            .lock()
            .unwrap()
            .entry(paper_id.to_string())
            .or_default()
            .clone();
        let _flight = gate.lock().await;
        let result = self.parse_inner(paper_id, refs, venue).await;
        let mut map = self.in_flight.lock().unwrap();
        // 2 = the map's Arc + our `gate`: nobody else is waiting on this
        // paper. A late waiter still holding the old gate just re-locks it;
        // at worst it races a brand-new flight, which is what happened on
        // every call before the guard existed.
        if map.get(paper_id).is_some_and(|g| Arc::strong_count(g) <= 2) {
            map.remove(paper_id);
        }
        result
    }

    /// The pipeline proper; `parse` holds the per-paper flight lock.
    async fn parse_inner(
        &self,
        paper_id: &str,
        refs: &[String],
        venue: Option<&str>,
    ) -> Result<Vec<Option<StructuredReference>>> {
        let refs_json = serde_json::to_string(refs)?;
        // An upgradeable row's keepable entries seed the re-parse so they
        // are never degraded.
        let mut seed: Option<Vec<Option<CachedReference>>> = None;
        if let Some((cached, provenance)) = store::get(&self.pool, paper_id, &refs_json).await? {
            let cached: Vec<Option<CachedReference>> = serde_json::from_str(&cached)?;
            let has_nulls = cached.iter().any(|p| p.is_none());
            // Three upgrade triggers:
            //  - an entry from an older heuristic generation (a
            //    HEURISTIC_SRC bump): the re-parse overwrites it, wrong
            //    `Some` included — nulls or no nulls;
            //  - a null-bearing row from an older parser generation (legacy
            //    pure-LLM rows): heuristics never saw it, so re-parse even
            //    without an LLM;
            //  - a null-bearing heuristics-only row once an LLM is
            //    configured.
            let older_generation = !provenance.starts_with(HEURISTIC_VERSION);
            let upgradeable = cached
                .iter()
                .flatten()
                .any(|c| stale_heuristic(c.src.as_deref()))
                || (has_nulls
                    && (older_generation
                        || (self.llm.is_some() && provenance == HEURISTIC_VERSION)));
            if !upgradeable {
                return Ok(cached.into_iter().map(|c| c.map(|c| c.reference)).collect());
            }
            seed = Some(cached);
        }
        let mut parsed = heuristic::parse_all(refs, venue);
        let mut srcs: Vec<Option<String>> = parsed
            .iter()
            .map(|p| p.as_ref().map(|_| HEURISTIC_SRC.to_string()))
            .collect();
        if let Some(seed) = seed {
            for ((slot, src), old) in parsed.iter_mut().zip(srcs.iter_mut()).zip(seed) {
                // A kept cached entry wins over a fresh heuristic parse (same
                // generation ⇒ same output anyway); only a stale heuristic
                // entry is dropped — re-derived, or retracted to null when
                // the current heuristics reject what an older one accepted.
                if let Some(old) = old {
                    if !stale_heuristic(old.src.as_deref()) {
                        *slot = Some(old.reference);
                        *src = old.src;
                    }
                }
            }
        }
        let leftover: Vec<usize> = parsed
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.is_none().then_some(i))
            .collect();
        let mut provenance = HEURISTIC_VERSION.to_string();
        if let Some(llm) = &self.llm {
            if !leftover.is_empty() {
                let leftover_refs: Vec<String> =
                    leftover.iter().map(|&i| refs[i].clone()).collect();
                match Self::parse_leftovers(llm, &leftover_refs).await {
                    Ok(results) => {
                        for (&slot, r) in leftover.iter().zip(results) {
                            srcs[slot] = r.as_ref().map(|_| format!("llm:{}", llm.model()));
                            parsed[slot] = r;
                        }
                        provenance = format!("{HEURISTIC_VERSION}+{}", llm.model());
                    }
                    Err(e) => {
                        // Partial result, cache skipped: the next open of
                        // this paper retries the LLM part.
                        tracing::warn!("citation LLM fallback failed: {e}");
                        return Ok(parsed);
                    }
                }
            }
        }
        let cached: Vec<Option<CachedReference>> = parsed
            .iter()
            .zip(srcs)
            .map(|(p, src)| {
                p.clone()
                    .map(|reference| CachedReference { src, reference })
            })
            .collect();
        store::upsert(
            &self.pool,
            paper_id,
            &refs_json,
            &serde_json::to_string(&cached)?,
            &provenance,
        )
        .await?;
        Ok(parsed)
    }

    /// Chunks are disjoint slices, so their LLM calls run concurrently — a
    /// ~100-entry bibliography's first open pays one round trip, not four.
    /// `try_join_all` preserves chunk order for the concatenation.
    async fn parse_leftovers(
        llm: &crate::llm::LlmClient,
        refs: &[String],
    ) -> Result<Vec<Option<StructuredReference>>> {
        let chunks = refs
            .chunks(CHUNK_SIZE)
            .map(|chunk| Self::parse_chunk(llm, chunk));
        let results = futures_util::future::try_join_all(chunks).await?;
        Ok(results.into_iter().flatten().collect())
    }

    /// One LLM call for one chunk. Results are INDEX-KEYED, not positional:
    /// models reliably fail exact-count array contracts (observed live even
    /// at 25 entries: 18 in → 19 out, messy extracted strings read as two
    /// entries). Each returned object names its entry via `"i"`; entries the
    /// model skips stay null, duplicates keep the first, out-of-range are
    /// dropped — a miscount can no longer poison the whole chunk.
    async fn parse_chunk(
        llm: &crate::llm::LlmClient,
        refs: &[String],
    ) -> Result<Vec<Option<StructuredReference>>> {
        let numbered: String = refs
            .iter()
            .enumerate()
            .map(|(i, r)| format!("{}. {}\n", i + 1, r))
            .collect();
        let user = format!(
            "Parse the numbered bibliography entries below. Return a JSON array of \
             objects, one per entry you can parse. Each object MUST be \
             {{\"i\":<the entry's number exactly as shown below>,\
             \"authors\":[\"Given Family\",...],\"title\":string|null,\"venue\":string|null,\
             \"year\":int|null,\"doi\":string|null,\"arxiv_id\":string|null,\"url\":string|null}}.\n\
             venue is the conference/journal name only. doi is the bare DOI (no https://). \
             arxiv_id is like \"1412.6980\".\n\
             SKIP entries that are not parseable bibliography references — do not \
             invent placeholders for them.\n\n{numbered}"
        );
        let text = llm.complete(SYSTEM, &user).await?;
        let cleaned = crate::llm::strip_code_fence(&text);
        let parsed: Vec<IndexedReference> = serde_json::from_str(cleaned)
            .map_err(|e| anyhow!("citation parse: model returned invalid JSON: {e}"))?;
        let mut out: Vec<Option<StructuredReference>> = vec![None; refs.len()];
        for item in parsed {
            if item.i >= 1 && item.i <= refs.len() && out[item.i - 1].is_none() {
                out[item.i - 1] = Some(item.reference);
            }
        }
        Ok(out)
    }
}

/// One element of the model's reply: a parsed entry tagged with the 1-based
/// chunk-local entry number it belongs to.
#[derive(Deserialize)]
struct IndexedReference {
    i: usize,
    #[serde(flatten)]
    reference: StructuredReference,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn chat_reply(text: &str) -> serde_json::Value {
        json!({"choices": [{"message": {"role": "assistant", "content": text}}]})
    }

    const PARSED: &str = r#"[{"i":1,"authors":["D. Kingma","J. Ba"],"title":"Adam: A Method for Stochastic Optimization","venue":"ICLR","year":2015,"doi":null,"arxiv_id":"1412.6980","url":null}]"#;

    #[tokio::test]
    async fn parses_via_llm_then_serves_from_cache() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("Kingma"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply(PARSED)))
            .expect(1) // second parse() must hit the cache
            .mount(&server)
            .await;
        let (pool, _dir) = crate::testutil::pool_with_paper("p1").await;
        let svc = CitationsService::for_tests(pool, &format!("{}/v1", server.uri()), "m");
        let refs = vec![
            "[1] D. Kingma, J. Ba. Adam...".to_string(),
            "garbage".to_string(),
        ];

        let out = svc.parse("p1", &refs, None).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].as_ref().unwrap().year, Some(2015));
        assert!(out[1].is_none());

        let again = svc.parse("p1", &refs, None).await.unwrap(); // no second LLM call (mock .expect(1))
        assert_eq!(again, out);
    }

    #[tokio::test]
    async fn malformed_llm_output_returns_partial_and_caches_nothing() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply("not json")))
            .mount(&server)
            .await;
        let (pool, _dir) = crate::testutil::pool_with_paper("p1").await;
        let svc = CitationsService::for_tests(pool.clone(), &format!("{}/v1", server.uri()), "m");
        // LLM failure → heuristic partial (here: all-None) with NO cache
        // write, so the next open retries.
        let out = svc.parse("p1", &["x".to_string()], None).await.unwrap();
        assert_eq!(out, vec![None]);
        assert!(store::get(&pool, "p1", r#"["x"]"#).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn chunks_large_batches_and_concatenates() {
        let server = MockServer::start().await;
        // 55 refs -> chunks of 25/25/5, one LLM call each. Chunks 2 and 3 are
        // recognized by their first entry; chunk 1 falls through to the
        // catch-all mounted last. Replies use chunk-local indexes; an entry
        // the model skips (or an empty reply) is simply null in the output.
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("REF-26 "))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply(
                r#"[{"i":1,"title":"chunk2-first","authors":[],"venue":null,"year":null,"doi":null,"arxiv_id":null,"url":null}]"#,
            )))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("REF-51 "))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply("[]")))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply("[]")))
            .expect(1)
            .mount(&server)
            .await;
        let refs: Vec<String> = (1..=55)
            .map(|i| format!("REF-{i} some paper title"))
            .collect();
        let (pool, _dir) = crate::testutil::pool_with_paper("p1").await;
        let svc = CitationsService::for_tests(pool.clone(), &format!("{}/v1", server.uri()), "m");

        let out = svc.parse("p1", &refs, None).await.unwrap();
        assert_eq!(out.len(), 55);
        // chunk-local index 1 of chunk 2 = overall entry 26
        assert_eq!(
            out[25].as_ref().unwrap().title.as_deref(),
            Some("chunk2-first")
        );
        assert!(out[0].is_none());
        let refs_json = serde_json::to_string(&refs).unwrap();
        assert!(store::get(&pool, "p1", &refs_json).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn tolerates_explicit_null_authors() {
        let server = MockServer::start().await;
        let reply = r#"[{"i":1,"authors":null,"title":"T","venue":null,"year":2020,"doi":null,"arxiv_id":null,"url":null}]"#;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply(reply)))
            .mount(&server)
            .await;
        let (pool, _dir) = crate::testutil::pool_with_paper("p1").await;
        let svc = CitationsService::for_tests(pool, &format!("{}/v1", server.uri()), "m");
        let out = svc.parse("p1", &["x".to_string()], None).await.unwrap();
        assert_eq!(out[0].as_ref().unwrap().title.as_deref(), Some("T"));
        assert!(out[0].as_ref().unwrap().authors.is_empty());
    }

    #[tokio::test]
    async fn unindexed_entries_become_null_and_bad_indexes_are_dropped() {
        let server = MockServer::start().await;
        // Model omits entry 2, duplicates entry 1, and invents entry 99:
        // entry 1 = first occurrence wins; entry 2 = null; 99 dropped.
        let reply = r#"[
            {"i":1,"title":"First","authors":[],"venue":null,"year":null,"doi":null,"arxiv_id":null,"url":null},
            {"i":1,"title":"Dup","authors":[],"venue":null,"year":null,"doi":null,"arxiv_id":null,"url":null},
            {"i":99,"title":"Ghost","authors":[],"venue":null,"year":null,"doi":null,"arxiv_id":null,"url":null}
        ]"#;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply(reply)))
            .mount(&server)
            .await;
        let (pool, _dir) = crate::testutil::pool_with_paper("p1").await;
        let svc = CitationsService::for_tests(pool, &format!("{}/v1", server.uri()), "m");
        let out = svc
            .parse("p1", &["a".to_string(), "b".to_string()], None)
            .await
            .unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].as_ref().unwrap().title.as_deref(), Some("First"));
        assert!(out[1].is_none());
    }

    const IEEE_REF: &str = r#"[1] K. Kim and T. Kim, "PGFUZZ: Policy-guided fuzzing for robotic vehicles," in Proceedings of NDSS, 2021."#;
    const IEEE_REF2: &str = r#"[2] D. Kingma and J. Ba, "Adam: A method for stochastic optimization," in Proc. of ICLR, 2015."#;
    const GARBLED: &str = r#"[3] %%GARBLED FRAGMENT%% "with a quote," but nothing else"#;

    #[tokio::test]
    async fn heuristics_parse_without_any_llm() {
        // No MockServer at all: heuristics must not do I/O.
        let (pool, _dir) = crate::testutil::pool_with_paper("p1").await;
        let svc = CitationsService::heuristic_only(pool.clone());
        let refs = vec![IEEE_REF.to_string(), IEEE_REF2.to_string()];
        let out = svc.parse("p1", &refs, None).await.unwrap();
        assert_eq!(
            out[0].as_ref().unwrap().title.as_deref(),
            Some("PGFUZZ: Policy-guided fuzzing for robotic vehicles")
        );
        assert_eq!(out[1].as_ref().unwrap().year, Some(2015));
        let (_, provenance) = store::get(&pool, "p1", &serde_json::to_string(&refs).unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(provenance, "heuristic-v1");
    }

    #[tokio::test]
    async fn llm_receives_only_the_leftovers() {
        let server = MockServer::start().await;
        // "1. [3] %%GARBLED…" proves the leftover list was renumbered from 1
        // — i.e. the two heuristic successes were NOT sent.
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("1. [3] %%GARBLED"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply(
                r#"[{"i":1,"authors":["G. Author"],"title":"Recovered Title","venue":null,"year":2020,"doi":null,"arxiv_id":null,"url":null}]"#,
            )))
            .expect(1)
            .mount(&server)
            .await;
        let (pool, _dir) = crate::testutil::pool_with_paper("p1").await;
        let svc = CitationsService::for_tests(pool.clone(), &format!("{}/v1", server.uri()), "m");
        let refs = vec![
            IEEE_REF.to_string(),
            IEEE_REF2.to_string(),
            GARBLED.to_string(),
        ];
        let out = svc.parse("p1", &refs, None).await.unwrap();
        assert!(out[0].is_some() && out[1].is_some()); // heuristic
        assert_eq!(
            out[2].as_ref().unwrap().title.as_deref(),
            Some("Recovered Title")
        );
        let (cached, provenance) = store::get(&pool, "p1", &serde_json::to_string(&refs).unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(provenance, "heuristic-v1+m");
        // Each entry records its own origin in the cache.
        let v: serde_json::Value = serde_json::from_str(&cached).unwrap();
        assert_eq!(v[0]["src"], "h1");
        assert_eq!(v[2]["src"], "llm:m");
    }

    #[tokio::test]
    async fn heuristic_only_cache_upgrades_once_llm_is_configured() {
        let (pool, _dir) = crate::testutil::pool_with_paper("p1").await;
        let refs = vec![IEEE_REF.to_string(), GARBLED.to_string()];
        // Pass 1: heuristics only → entry 2 is null, cached as heuristic-v1.
        let out = CitationsService::heuristic_only(pool.clone())
            .parse("p1", &refs, None)
            .await
            .unwrap();
        assert!(out[1].is_none());
        // Pass 2: LLM configured → the heuristic-v1+nulls row reparses.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply(
                r#"[{"i":1,"authors":["G. Author"],"title":"Recovered Title","venue":null,"year":2020,"doi":null,"arxiv_id":null,"url":null}]"#,
            )))
            .expect(1) // pass 3 below must be a cache hit
            .mount(&server)
            .await;
        let svc = CitationsService::for_tests(pool.clone(), &format!("{}/v1", server.uri()), "m");
        let out2 = svc.parse("p1", &refs, None).await.unwrap();
        assert!(out2[1].is_some());
        // Pass 3: upgraded row (provenance has +m) is a plain cache hit.
        let out3 = svc.parse("p1", &refs, None).await.unwrap();
        assert_eq!(out3, out2);
    }

    #[tokio::test]
    async fn legacy_rows_with_nulls_reparse_and_keep_llm_entries() {
        // A cache row from the pure-LLM era (model column = raw model name)
        // containing nulls was written before heuristics existed (live bug:
        // the old model-miscount era dropped 60% of a paper's entries and the
        // row was served forever). Such rows re-parse once with the current
        // generation: heuristics fill what they can, but cached NON-null
        // entries are kept — a legacy LLM parse is never degraded.
        let (pool, _dir) = crate::testutil::pool_with_paper("p1").await;
        // GARBLED votes Ieee (quoted) but fails validation, so the style vote
        // clears 60% while the entry itself stays heuristically unparseable.
        let refs = vec![IEEE_REF.to_string(), GARBLED.to_string()];
        let refs_json = serde_json::to_string(&refs).unwrap();
        let legacy = r#"[null,{"authors":["L. Legacy"],"title":"Legacy Keep","venue":null,"year":2020,"doi":null,"arxiv_id":null,"url":null}]"#;
        store::upsert(&pool, "p1", &refs_json, legacy, "gpt-4o-mini")
            .await
            .unwrap();
        // No LLM at all: the upgrade must still fire (heuristics need no LLM).
        let out = CitationsService::heuristic_only(pool.clone())
            .parse("p1", &refs, None)
            .await
            .unwrap();
        assert_eq!(
            out[0].as_ref().unwrap().title.as_deref(),
            Some("PGFUZZ: Policy-guided fuzzing for robotic vehicles")
        );
        assert_eq!(
            out[1].as_ref().unwrap().title.as_deref(),
            Some("Legacy Keep")
        );
        let (cached, provenance) = store::get(&pool, "p1", &refs_json).await.unwrap().unwrap();
        assert_eq!(provenance, "heuristic-v1"); // row rewritten to current gen
                                                // Per-entry provenance on the rewrite: the fresh parse is tagged with
                                                // the current generation; the kept legacy entry stays origin-unknown
                                                // (no src), so no future generation bump will overwrite it either.
        let v: serde_json::Value = serde_json::from_str(&cached).unwrap();
        assert_eq!(v[0]["src"], "h1");
        assert!(v[1].get("src").is_none());
    }

    #[tokio::test]
    async fn a_generation_bump_reparses_stale_heuristic_entries_and_keeps_llm_ones() {
        // A fully-parsed row from an older heuristic generation: the old
        // has_nulls-gated upgrade would have served it forever, wrong `Some`
        // included. Per-entry provenance re-parses exactly the h* entries
        // and leaves the LLM one untouched.
        let (pool, _dir) = crate::testutil::pool_with_paper("p1").await;
        let refs = vec![IEEE_REF.to_string(), GARBLED.to_string()];
        let refs_json = serde_json::to_string(&refs).unwrap();
        let stale = r#"[{"src":"h0","authors":["W. Wrong"],"title":"A Wrong Title","venue":null,"year":2000,"doi":null,"arxiv_id":null,"url":null},{"src":"llm:m","authors":["G. Author"],"title":"LLM Keep","venue":null,"year":2020,"doi":null,"arxiv_id":null,"url":null}]"#;
        store::upsert(&pool, "p1", &refs_json, stale, "heuristic-v0+m")
            .await
            .unwrap();
        let out = CitationsService::heuristic_only(pool.clone())
            .parse("p1", &refs, None)
            .await
            .unwrap();
        assert_eq!(
            out[0].as_ref().unwrap().title.as_deref(),
            Some("PGFUZZ: Policy-guided fuzzing for robotic vehicles"),
            "the stale heuristic parse is re-derived"
        );
        assert_eq!(out[1].as_ref().unwrap().title.as_deref(), Some("LLM Keep"));
        let (_, provenance) = store::get(&pool, "p1", &refs_json).await.unwrap().unwrap();
        assert_eq!(provenance, "heuristic-v1");
    }

    #[tokio::test]
    async fn a_generation_bump_retracts_a_false_some_the_new_heuristics_reject() {
        // The bump's whole purpose: a wrong `Some` from an older generation
        // must not survive just because it is non-null. Current heuristics
        // can't parse GARBLED at all, so the entry goes back to null (and
        // would go to the LLM leg, were one configured).
        let (pool, _dir) = crate::testutil::pool_with_paper("p1").await;
        let refs = vec![GARBLED.to_string()];
        let refs_json = serde_json::to_string(&refs).unwrap();
        let stale = r#"[{"src":"h0","authors":["W. Wrong"],"title":"A Wrong Title","venue":null,"year":2000,"doi":null,"arxiv_id":null,"url":null}]"#;
        store::upsert(&pool, "p1", &refs_json, stale, "heuristic-v0")
            .await
            .unwrap();
        let out = CitationsService::heuristic_only(pool.clone())
            .parse("p1", &refs, None)
            .await
            .unwrap();
        assert_eq!(out, vec![None]);
        let (cached, provenance) = store::get(&pool, "p1", &refs_json).await.unwrap().unwrap();
        assert_eq!(cached, "[null]");
        assert_eq!(provenance, "heuristic-v1");
    }

    #[tokio::test]
    async fn concurrent_parses_of_one_paper_share_a_single_flight() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply(PARSED)))
            .expect(1) // the second caller waits, then hits the cache
            .mount(&server)
            .await;
        let (pool, _dir) = crate::testutil::pool_with_paper("p1").await;
        let svc = CitationsService::for_tests(pool, &format!("{}/v1", server.uri()), "m");
        let refs = vec!["[1] D. Kingma, J. Ba. Adam...".to_string()];
        let (a, b) = tokio::join!(svc.parse("p1", &refs, None), svc.parse("p1", &refs, None));
        assert_eq!(a.unwrap(), b.unwrap());
    }

    #[tokio::test]
    async fn legacy_rows_without_nulls_are_served_as_is() {
        // A fully-parsed legacy row has nothing to gain from re-parsing —
        // serve it untouched (and never re-call the LLM for it).
        let (pool, _dir) = crate::testutil::pool_with_paper("p1").await;
        let refs = vec!["x".to_string()]; // heuristically unparseable
        let refs_json = serde_json::to_string(&refs).unwrap();
        let legacy = r#"[{"authors":["L. Legacy"],"title":"Legacy Keep","venue":null,"year":2020,"doi":null,"arxiv_id":null,"url":null}]"#;
        store::upsert(&pool, "p1", &refs_json, legacy, "gpt-4o-mini")
            .await
            .unwrap();
        let server = MockServer::start().await; // no mocks: an LLM call would 404 -> Err
        let svc = CitationsService::for_tests(pool.clone(), &format!("{}/v1", server.uri()), "m");
        let out = svc.parse("p1", &refs, None).await.unwrap();
        assert_eq!(
            out[0].as_ref().unwrap().title.as_deref(),
            Some("Legacy Keep")
        );
        let (_, provenance) = store::get(&pool, "p1", &refs_json).await.unwrap().unwrap();
        assert_eq!(provenance, "gpt-4o-mini"); // row untouched
    }

    #[tokio::test]
    async fn llm_failure_during_upgrade_preserves_old_cache_row() {
        let (pool, _dir) = crate::testutil::pool_with_paper("p1").await;
        let refs = vec![IEEE_REF.to_string(), GARBLED.to_string()];
        // Seed an upgradeable row: heuristics-only provenance with a null.
        CitationsService::heuristic_only(pool.clone())
            .parse("p1", &refs, None)
            .await
            .unwrap();
        let refs_json = serde_json::to_string(&refs).unwrap();
        let before = store::get(&pool, "p1", &refs_json).await.unwrap().unwrap();
        assert_eq!(before.1, "heuristic-v1");
        // LLM configured but broken (no mock -> 404): upgrade attempt must
        // return the fresh heuristic partial and leave the old row intact.
        let server = MockServer::start().await;
        let svc = CitationsService::for_tests(pool.clone(), &format!("{}/v1", server.uri()), "m");
        let out = svc.parse("p1", &refs, None).await.unwrap();
        assert!(out[0].is_some() && out[1].is_none());
        let after = store::get(&pool, "p1", &refs_json).await.unwrap().unwrap();
        assert_eq!(after, before);
    }
}
