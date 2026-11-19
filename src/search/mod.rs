pub(crate) mod chunker;
pub mod embedder;
pub mod fts;
mod fusion;
pub mod indexer;
mod planner;
pub(crate) mod query;
mod semantic;
mod snippet;
mod store;
pub mod vector;

pub(crate) use semantic::Semantic;
pub use semantic::SemanticTier;
pub use snippet::{Snippet, SnippetPart};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use ts_rs::TS;

use crate::config::SearchConfig;
use crate::library::model::Paper;

const KEYWORD_LIMIT: usize = 100;
const SEMANTIC_LIMIT: usize = 50;
const RRF_K: f32 = 60.0;
const SEMANTIC_SNIPPET_CHARS: usize = 200;

pub struct SearchRequest {
    pub q: String,
    /// `author:` qualifier terms — each a phrase on the authors field, ANDed
    /// with the free text in `q`.
    pub author_terms: Vec<String>,
    pub fields: fts::FieldSel,
    pub keyword: bool,
    pub semantic: bool,
    pub status: Option<crate::library::model::PaperStatus>,
    pub project: Option<String>,
    pub tag: Option<String>,
    pub starred: Option<bool>,
}

/// Caller-supplied inputs to `SearchRequest::assemble` that don't come from
/// the query string: the engine selection plus fallback filters that parsed
/// qualifiers override (the web handler forwards its query parameters; the
/// CLI has no equivalents and passes `None`s).
pub(crate) struct RequestOverrides {
    pub(crate) keyword: bool,
    pub(crate) semantic: bool,
    /// CSV field list, used only when the query has no `in:` qualifier.
    pub(crate) fields: Option<String>,
    pub(crate) status: Option<crate::library::model::PaperStatus>,
    /// Project id (not name — `project:` NAMEs live in the query string).
    pub(crate) project: Option<String>,
    pub(crate) tag: Option<String>,
    pub(crate) starred: Option<bool>,
}

impl SearchRequest {
    /// The one place a raw query string becomes a `SearchRequest` (shared by
    /// `/api/search` and `xuewen search`): parse it, resolve a `project:`
    /// NAME to its id, and merge parsed qualifiers over `overrides`
    /// (qualifiers win). An unknown project name binds the name itself — it
    /// can never equal a project id, so the search filters to zero results;
    /// a *failing* lookup propagates instead of being mistaken for that.
    pub(crate) async fn assemble(
        pool: &SqlitePool,
        raw_q: &str,
        overrides: RequestOverrides,
    ) -> Result<Self> {
        let parsed = query::parse(raw_q);
        let project = match parsed.project {
            Some(name) => Some(
                crate::library::projects::find_by_name(pool, &name)
                    .await?
                    .map(|p| p.id)
                    .unwrap_or(name),
            ),
            None => overrides.project,
        };
        Ok(Self {
            q: parsed.text,
            author_terms: parsed.authors,
            fields: parsed
                .fields
                .unwrap_or_else(|| fts::FieldSel::parse(overrides.fields.as_deref())),
            keyword: overrides.keyword,
            semantic: overrides.semantic,
            status: parsed.status.or(overrides.status),
            project,
            tag: parsed.tag.or(overrides.tag),
            starred: if parsed.starred {
                Some(true)
            } else {
                overrides.starred
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, rename = "SemanticAvailability")]
pub struct SemanticState {
    pub available: bool,
    pub reason: Option<String>,
}

impl SemanticState {
    fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            available: false,
            reason: Some(reason.into()),
        }
    }
}

/// Which engine(s) matched a paper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, rename = "SearchEngine")]
pub enum Engine {
    Keyword,
    Semantic,
    Both,
}

/// Why a paper matched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, rename = "SearchMatch")]
pub struct MatchInfo {
    pub engine: Engine,
    pub field: fts::FieldName,
    pub snippet: Snippet,
    pub page: Option<i64>,
}

/// One hit, with the paper in whatever shape the consumer shows it: the
/// service returns the `Paper`, `/api/search` a row summary, and the CLI
/// reads back only the labels it prints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, rename = "SearchResultItem")]
pub struct SearchHit<P> {
    pub paper: P,
    #[serde(rename = "match")]
    pub matched: MatchInfo,
}

/// The body of `GET /api/search`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, rename = "SearchResponse")]
pub struct SearchResults<P> {
    pub semantic: SemanticState,
    pub results: Vec<SearchHit<P>>,
}

impl<P> SearchResults<P> {
    pub(crate) fn map_papers<Q>(self, mut f: impl FnMut(P) -> Q) -> SearchResults<Q> {
        SearchResults {
            semantic: self.semantic,
            results: self
                .results
                .into_iter()
                .map(|h| SearchHit {
                    paper: f(h.paper),
                    matched: h.matched,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TierCounts {
    pub(crate) indexed: i64,
    pub pending: i64,
    pub failed: i64,
}

/// The body of `GET /api/search/status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, rename = "SearchStatus")]
pub struct IndexStatus {
    pub fts: TierCounts,
    pub(crate) vectors: TierCounts,
    pub(crate) semantic_available: bool,
    pub(crate) reason: Option<String>,
}

/// Owns the three search backends. SQLite remains the source of truth;
/// Tantivy and the vector store are derived and rebuildable.
pub struct SearchService {
    pool: SqlitePool,
    /// Arc so the spawn_blocking facades below can move a handle into their
    /// closures; the index itself stays synchronous.
    pub fts: Arc<fts::FtsIndex>,
    semantic: Semantic,
    notify: tokio::sync::Notify,
}

impl SearchService {
    pub(crate) async fn open(
        pool: SqlitePool,
        cfg: &SearchConfig,
        semantic: Semantic,
    ) -> Result<Arc<Self>> {
        let (fts_idx, created) = fts::FtsIndex::open(&cfg.index_dir)?;
        if created {
            store::clear_stamps(&pool, true, false).await?;
        }
        Ok(Arc::new(Self {
            pool,
            fts: Arc::new(fts_idx),
            semantic,
            notify: tokio::sync::Notify::new(),
        }))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn open_with(
        pool: SqlitePool,
        fts: fts::FtsIndex,
        semantic: Option<SemanticTier>,
    ) -> Arc<Self> {
        Arc::new(Self {
            pool,
            fts: Arc::new(fts),
            semantic: semantic.into(),
            notify: tokio::sync::Notify::new(),
        })
    }

    /// Tantivy's writes, commits and searches do real fsync-backed disk I/O;
    /// this runs them on the blocking pool so a commit-heavy sweep or a
    /// large-segment search can't stall the tokio workers serving HTTP.
    async fn on_fts<T: Send + 'static>(
        &self,
        what: &'static str,
        op: impl FnOnce(&fts::FtsIndex) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let fts = Arc::clone(&self.fts);
        tokio::task::spawn_blocking(move || op(&fts))
            .await
            .with_context(|| format!("fts {what} task panicked"))?
    }

    async fn fts_search(
        &self,
        q: String,
        authors: Vec<String>,
        sel: fts::FieldSel,
        scope: Option<Vec<String>>,
    ) -> Result<Vec<fts::FtsHit>> {
        self.on_fts("search", move |fts| {
            fts.search(&q, &authors, &sel, KEYWORD_LIMIT, scope.as_deref())
        })
        .await
    }

    pub(crate) async fn fts_stage_upsert(&self, doc: fts::PaperDoc) -> Result<()> {
        self.on_fts("upsert", move |fts| fts.stage_upsert(&doc))
            .await
    }

    pub(crate) async fn fts_stage_deletes(&self, paper_ids: Vec<String>) -> Result<()> {
        self.on_fts("delete", move |fts| {
            paper_ids.iter().try_for_each(|id| fts.stage_delete(id))
        })
        .await
    }

    pub(crate) async fn fts_commit(&self) -> Result<()> {
        self.on_fts("commit", |fts| fts.commit()).await
    }

    pub(crate) async fn claim_writer(&self) -> Result<fts::WriterClaim> {
        self.on_fts("writer claim", |fts| fts.claim_writer()).await
    }

    pub(crate) fn release_writer(&self) {
        self.fts.release_writer();
    }

    pub(crate) async fn close_vectors(&self) {
        if let Some(vectors) = self.semantic.store() {
            vectors.close_gracefully().await;
        }
    }

    /// Nudge the indexer to sweep now (harmless if nothing is stale).
    pub(crate) fn wake(&self) {
        self.notify.notify_one();
    }

    fn semantic_config_state(&self) -> SemanticState {
        let reason = self.semantic.reason();
        SemanticState {
            available: reason.is_none(),
            reason,
        }
    }

    pub async fn search(&self, req: &SearchRequest) -> Result<SearchResults<Paper>> {
        let q = req.q.trim();
        let mut semantic = self.semantic_config_state();
        // Only title/abstract/body are embedded; a query scoped to authors or
        // notes alone has nothing for the vector tier to match.
        if !req.fields.semantic_applicable() && semantic.available {
            semantic = SemanticState::unavailable(
                "semantic search does not apply to a query scoped to authors or notes",
            );
        }
        if q.is_empty() && !req.author_terms.is_empty() && semantic.available {
            semantic = SemanticState::unavailable(
                "semantic search does not apply to an authors-only query",
            );
        }

        // Resolve org filters to a paper-id scope BEFORE either engine runs:
        // each engine truncates to its top K, so a filter applied after the
        // cutoff would silently drop matches ranked past it exactly when a
        // qualifier narrows a broad term.
        let filter = crate::library::papers::PaperFilter {
            status: req.status,
            project: req.project.clone(),
            tag: req.tag.clone(),
            starred: req.starred,
        };
        let filtered = filter.status.is_some()
            || filter.project.is_some()
            || filter.tag.is_some()
            || filter.starred == Some(true);
        let scope: Option<Vec<String>> = if filtered {
            Some(store::filtered_paper_ids(&self.pool, &filter).await?)
        } else {
            None
        };
        if scope.as_ref().is_some_and(|ids| ids.is_empty()) {
            // Nothing satisfies the filter; don't bother either engine.
            return Ok(SearchResults {
                semantic,
                results: Vec::new(),
            });
        }

        let keyword_hits = if req.keyword {
            self.fts_search(
                q.to_string(),
                req.author_terms.clone(),
                req.fields,
                scope.clone(),
            )
            .await?
        } else {
            Vec::new()
        };

        // Best chunk per paper, in vector score order.
        let mut semantic_best: Vec<vector::VecHit> = Vec::new();
        if req.semantic && semantic.available && !q.is_empty() {
            if let Some(tier) = self.semantic.tier() {
                match Self::semantic_search(tier, q, &req.fields, scope.as_deref()).await {
                    Ok(hits) => {
                        let mut seen = std::collections::HashSet::new();
                        for h in hits {
                            if seen.insert(h.paper_id.clone()) {
                                semantic_best.push(h);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("semantic search failed: {e}");
                        semantic = SemanticState::unavailable(e.to_string());
                    }
                }
            }
        }

        let keyword_ids: Vec<String> = keyword_hits.iter().map(|h| h.paper_id.clone()).collect();
        let semantic_ids: Vec<String> = semantic_best.iter().map(|h| h.paper_id.clone()).collect();
        let fused = fusion::rrf(&[keyword_ids, semantic_ids], RRF_K);

        let papers = store::papers_by_ids_ordered(&self.pool, &fused).await?;

        let mut kw_by_id: HashMap<&str, &fts::FtsHit> = keyword_hits
            .iter()
            .map(|h| (h.paper_id.as_str(), h))
            .collect();
        let semantic_only: Vec<&vector::VecHit> = semantic_best
            .iter()
            .filter(|h| !kw_by_id.contains_key(h.paper_id.as_str()))
            .collect();
        let mut semantic_matches = self.semantic_matches(&semantic_only).await?;
        let semantic_ids: std::collections::HashSet<&str> =
            semantic_best.iter().map(|h| h.paper_id.as_str()).collect();

        let mut results = Vec::with_capacity(papers.len());
        for paper in papers {
            let matched = match kw_by_id.remove(paper.id.as_str()) {
                Some(k) => MatchInfo {
                    engine: if semantic_ids.contains(paper.id.as_str()) {
                        Engine::Both
                    } else {
                        Engine::Keyword
                    },
                    field: k.field,
                    snippet: k.snippet.clone(),
                    page: None,
                },
                // A semantic hit whose chunk text is gone (re-chunked since
                // the vector was written) has nothing to show.
                None => match semantic_matches.remove(paper.id.as_str()) {
                    Some(m) => m,
                    None => continue,
                },
            };
            results.push(SearchHit { paper, matched });
        }
        Ok(SearchResults { semantic, results })
    }

    async fn semantic_search(
        tier: &SemanticTier,
        q: &str,
        sel: &fts::FieldSel,
        scope: Option<&[String]>,
    ) -> Result<Vec<vector::VecHit>> {
        let vecs = tier.embedder.embed(&[q.to_string()]).await?;
        let filter = match (sel.title || sel.abstract_text, sel.body) {
            (true, true) => vector::SeqFilter::All,
            (false, true) => vector::SeqFilter::OnlyBody,
            (true, false) => vector::SeqFilter::OnlySummary,
            (false, false) => vector::SeqFilter::All, // authors-only never reaches here
        };
        tier.store
            .search(&vecs[0], SEMANTIC_LIMIT, filter, scope)
            .await
    }

    /// Snippets for semantic-only hits: each matching chunk's text, trimmed.
    /// Keyed by paper id; a hit whose chunk is missing gets none.
    async fn semantic_matches(
        &self,
        hits: &[&vector::VecHit],
    ) -> Result<HashMap<String, MatchInfo>> {
        let keys: Vec<(String, i64)> = hits.iter().map(|h| (h.paper_id.clone(), h.seq)).collect();
        let mut texts = store::chunk_texts(&self.pool, &keys).await?;
        Ok(hits
            .iter()
            .filter_map(|hit| {
                let text = texts.remove(&(hit.paper_id.clone(), hit.seq))?;
                let mut shown: String = text.chars().take(SEMANTIC_SNIPPET_CHARS).collect();
                if shown.len() < text.len() {
                    shown.push('…');
                }
                let (field, page) = if hit.seq == 0 {
                    (fts::FieldName::Abstract, None)
                } else {
                    (fts::FieldName::Body, hit.page)
                };
                let info = MatchInfo {
                    engine: Engine::Semantic,
                    field,
                    snippet: Snippet::plain(&shown),
                    page,
                };
                Some((hit.paper_id.clone(), info))
            })
            .collect())
    }

    /// Planner input for every paper — deliberately unfiltered, trashed rows
    /// included: the planner needs them (as `trashed`) to detect tombstones
    /// still holding index state.
    async fn paper_states(&self) -> Result<Vec<planner::PaperState>> {
        let papers = sqlx::query_as::<_, Paper>("SELECT * FROM papers")
            .fetch_all(&self.pool)
            .await?;
        // One query for every paper's notes rather than one per paper: most
        // papers have none, and this runs on every sweep.
        let notes = crate::annotations::store::notes_by_paper(&self.pool).await?;
        Ok(papers
            .iter()
            .map(|p| planner::PaperState {
                id: p.id.clone(),
                content_hash: p.content_hash.clone(),
                summary_hash: store::summary_hash(p),
                doc_hash: store::doc_hash(p, notes.get(&p.id).map_or("", String::as_str)),
                trashed: p.deleted_at.is_some(),
            })
            .collect())
    }

    pub async fn status(&self) -> Result<IndexStatus> {
        let papers = self.paper_states().await?;
        let rows = store::all_index_rows(&self.pool).await?;
        let by_id: HashMap<&str, &store::IndexRow> =
            rows.iter().map(|r| (r.paper_id.as_str(), r)).collect();
        let model = self.semantic.tier().map(|t| t.embedder.model());

        // A tier is "indexed" exactly when the planner's staleness predicate
        // sees no work for it — one definition, so status and the sweep can
        // never disagree about what "indexed" means. Backoff is deliberately
        // absent here: a failed-and-waiting paper still counts as pending.
        // "Failed" is gated on the same staleness: an error on a tier with no
        // pending work is residue no pass will ever run to clear (a pre-split
        // failure charged to a vector tier that never runs keyword-only, or
        // migration 0023's copy landing on a tier that was already current)
        // and must not be reported forever.
        let (mut fts_indexed, mut vec_indexed, mut live_n) = (0i64, 0i64, 0i64);
        let (mut fts_failed, mut vec_failed) = (0i64, 0i64);
        for p in papers.iter().filter(|p| !p.trashed) {
            live_n += 1;
            let row = by_id.get(p.id.as_str()).copied();
            let stale = planner::tier_staleness(p, row, model);
            if !stale.fts {
                fts_indexed += 1;
            } else if row.is_some_and(|r| r.fts_last_error.is_some()) {
                fts_failed += 1;
            }
            if stale.vectors == planner::VectorWork::None {
                vec_indexed += 1;
            } else if row.is_some_and(|r| r.vec_last_error.is_some()) {
                vec_failed += 1;
            }
        }
        let sem = self.semantic_config_state();
        // Without an embedder the vectors tier is idle, not "all indexed".
        let vectors = if self.semantic.tier().is_some() {
            TierCounts {
                indexed: vec_indexed,
                pending: live_n - vec_indexed,
                failed: vec_failed,
            }
        } else {
            TierCounts {
                indexed: 0,
                pending: 0,
                failed: vec_failed,
            }
        };
        Ok(IndexStatus {
            fts: TierCounts {
                indexed: fts_indexed,
                pending: live_n - fts_indexed,
                failed: fts_failed,
            },
            vectors,
            semantic_available: sem.available,
            reason: sem.reason,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::model::{Authors, Paper, PaperMeta, PaperStatus};
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn paper(id: &str, title: &str) -> Paper {
        Paper {
            id: id.into(),
            content_hash: format!("hash-{id}"),
            rel_path: format!("{id}.pdf"),
            cite_key: None,
            added_at: "2026-07-09T00:00:00Z".into(),
            deleted_at: None,
            starred: false,
            name: None,
            meta: PaperMeta {
                title: Some(title.into()),
                abstract_text: None,
                authors: Authors(vec!["Ada Lovelace".into()]),
                venue: None,
                year: Some(2026),
                doi: None,
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: PaperStatus::Resolved,
            },
        }
    }

    #[tokio::test]
    async fn assemble_merges_qualifiers_over_overrides_and_resolves_project() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        let proj = crate::library::projects::create(
            &pool,
            &crate::library::projects::ProjectName::parse("Thesis").unwrap(),
        )
        .await
        .unwrap();

        // Qualifiers win over the fallback params; `project:` resolves by
        // name, case-insensitively.
        let req = SearchRequest::assemble(
            &pool,
            "fuzzing tag:nlp is:starred project:thesis status:resolved in:title",
            RequestOverrides {
                keyword: true,
                semantic: false,
                fields: Some("body".into()),
                status: Some(crate::library::model::PaperStatus::NeedsReview),
                project: Some("fallback-id".into()),
                tag: Some("systems".into()),
                starred: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(req.q, "fuzzing");
        assert_eq!(req.project.as_deref(), Some(proj.id.as_str()));
        assert_eq!(req.tag.as_deref(), Some("nlp"));
        assert_eq!(
            req.status,
            Some(crate::library::model::PaperStatus::Resolved)
        );
        assert_eq!(req.starred, Some(true));
        assert!(req.fields.title && !req.fields.body);
        assert!(req.keyword && !req.semantic);

        // No qualifiers -> the fallbacks flow through untouched.
        let req = SearchRequest::assemble(
            &pool,
            "fuzzing",
            RequestOverrides {
                keyword: true,
                semantic: true,
                fields: Some("body".into()),
                status: Some(crate::library::model::PaperStatus::NeedsReview),
                project: Some("explicit-id".into()),
                tag: Some("systems".into()),
                starred: Some(true),
            },
        )
        .await
        .unwrap();
        assert_eq!(req.project.as_deref(), Some("explicit-id"));
        assert_eq!(req.tag.as_deref(), Some("systems"));
        assert_eq!(
            req.status,
            Some(crate::library::model::PaperStatus::NeedsReview)
        );
        assert_eq!(req.starred, Some(true));
        assert!(req.fields.body && !req.fields.title);

        // Unknown project NAME binds the name (filters to zero) — not an error.
        let req = SearchRequest::assemble(
            &pool,
            "project:nosuch",
            RequestOverrides {
                keyword: true,
                semantic: true,
                fields: None,
                status: None,
                project: None,
                tag: None,
                starred: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(req.project.as_deref(), Some("nosuch"));
    }

    /// Service with keyword tier real (temp Tantivy), semantic unavailable.
    async fn keyword_only_service(
        pool: sqlx::SqlitePool,
    ) -> (tempfile::TempDir, std::sync::Arc<SearchService>) {
        let dir = tempfile::tempdir().unwrap();
        let (fts, _) = fts::FtsIndex::open(dir.path()).unwrap();
        (dir, SearchService::open_with(pool, fts, None))
    }

    async fn tier(
        server: &MockServer,
        seeds: Vec<vector::ChunkPoint>,
    ) -> (tempfile::TempDir, SemanticTier) {
        crate::testutil::tier(&format!("{}/v1", server.uri()), seeds).await
    }

    /// A chunk whose vector is what the mocked embedder returns for any query.
    fn seed(paper: &str, seq: i64, page: Option<i64>) -> vector::ChunkPoint {
        crate::testutil::chunk(paper, seq, page, vec![0.1, 0.2, 0.3, 0.4])
    }

    #[tokio::test]
    async fn keyword_search_returns_papers_with_snippets_in_rank_order() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        for (id, title) in [("a", "Fuzzing Firmware"), ("b", "Sorting Networks")] {
            crate::library::papers::insert(&pool, &paper(id, title))
                .await
                .unwrap();
        }
        let (_index, svc) = keyword_only_service(pool).await;
        svc.fts
            .upsert(&fts::PaperDoc {
                id: "a".into(),
                title: "Fuzzing Firmware".into(),
                authors: "Ada Lovelace".into(),
                venue: String::new(),
                abstract_text: String::new(),
                body: "we fuzz routers".into(),
                notes: String::new(),
            })
            .unwrap();
        svc.fts
            .upsert(&fts::PaperDoc {
                id: "b".into(),
                title: "Sorting Networks".into(),
                authors: "Ada Lovelace".into(),
                venue: String::new(),
                abstract_text: String::new(),
                body: "batcher merge".into(),
                notes: String::new(),
            })
            .unwrap();

        let out = svc
            .search(&SearchRequest {
                q: "fuzzing".into(),
                author_terms: Vec::new(),
                fields: fts::FieldSel::all(),
                keyword: true,
                semantic: true,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();

        assert!(!out.semantic.available); // no embedder configured
        assert!(out.semantic.reason.is_some());
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].paper.id, "a");
        assert_eq!(out.results[0].matched.engine, Engine::Keyword);
        assert!(out.results[0].matched.snippet.0.iter().any(|p| p.marked));
    }

    #[tokio::test]
    async fn trashed_papers_are_filtered_at_hydration() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("a", "Fuzzing Firmware"))
            .await
            .unwrap();
        crate::library::trash::soft_delete(&pool, "a")
            .await
            .unwrap();
        let (_index, svc) = keyword_only_service(pool).await;
        svc.fts
            .upsert(&fts::PaperDoc {
                id: "a".into(),
                title: "Fuzzing Firmware".into(),
                authors: String::new(),
                venue: String::new(),
                abstract_text: String::new(),
                body: String::new(),
                notes: String::new(),
            })
            .unwrap();
        let out = svc
            .search(&SearchRequest {
                q: "fuzzing".into(),
                author_terms: Vec::new(),
                fields: fts::FieldSel::all(),
                keyword: true,
                semantic: false,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();
        assert!(
            out.results.is_empty(),
            "trashed paper leaked through hydration"
        );
    }

    #[tokio::test]
    async fn org_filters_scope_the_engines_and_short_circuit_when_empty() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        for (id, title) in [("a", "Fuzzing Firmware"), ("b", "Fuzzing Kernels")] {
            crate::library::papers::insert(&pool, &paper(id, title))
                .await
                .unwrap();
        }
        crate::library::tags::add_paper(
            &pool,
            "a",
            &crate::library::tags::TagName::parse("nlp").unwrap(),
        )
        .await
        .unwrap();
        let (_index, svc) = keyword_only_service(pool).await;
        for (id, title) in [("a", "Fuzzing Firmware"), ("b", "Fuzzing Kernels")] {
            svc.fts
                .upsert(&fts::PaperDoc {
                    id: id.into(),
                    title: title.into(),
                    authors: String::new(),
                    venue: String::new(),
                    abstract_text: String::new(),
                    body: String::new(),
                    notes: String::new(),
                })
                .unwrap();
        }
        let search_with_tag = |tag: &str| SearchRequest {
            q: "fuzzing".into(),
            author_terms: Vec::new(),
            fields: fts::FieldSel::all(),
            keyword: true,
            semantic: false,
            status: None,
            project: None,
            tag: Some(tag.into()),
            starred: None,
        };

        let out = svc.search(&search_with_tag("nlp")).await.unwrap();
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].paper.id, "a");

        // A filter nothing satisfies resolves to an empty id scope, which
        // short-circuits before either engine runs.
        let out = svc.search(&search_with_tag("nosuch")).await.unwrap();
        assert!(out.results.is_empty());
    }

    #[tokio::test]
    async fn hybrid_search_fuses_and_marks_both() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("a", "Fuzzing Firmware"))
            .await
            .unwrap();
        // Chunk for the semantic snippet lookup.
        crate::search::store::replace_body(
            &pool,
            "a",
            &[crate::search::chunker::Chunk {
                seq: 1,
                page: Some(7),
                text: "router fuzz harness details".into(),
            }],
            "hash-a",
        )
        .await
        .unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"index": 0, "embedding": [0.1, 0.2, 0.3, 0.4]}]
            })))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(dir.path()).unwrap();
        fts_idx
            .upsert(&fts::PaperDoc {
                id: "a".into(),
                title: "Fuzzing Firmware".into(),
                authors: String::new(),
                venue: String::new(),
                abstract_text: String::new(),
                body: "we fuzz routers".into(),
                notes: String::new(),
            })
            .unwrap();
        let (_vectors, tier) = tier(&server, vec![seed("a", 1, Some(7))]).await;
        let svc = SearchService::open_with(pool, fts_idx, Some(tier));

        let out = svc
            .search(&SearchRequest {
                q: "fuzzing".into(),
                author_terms: Vec::new(),
                fields: fts::FieldSel::all(),
                keyword: true,
                semantic: true,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();

        assert!(out.semantic.available);
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].matched.engine, Engine::Both);
        assert!(
            out.results[0].matched.snippet.0.iter().any(|p| p.marked),
            "keyword snippet preferred"
        );
    }

    #[tokio::test]
    async fn semantic_only_hit_uses_chunk_text_snippet() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("a", "Some Paper"))
            .await
            .unwrap();
        crate::search::store::replace_body(
            &pool,
            "a",
            &[crate::search::chunker::Chunk {
                seq: 2,
                page: Some(3),
                text: "novel <escaping> content".into(),
            }],
            "hash-a",
        )
        .await
        .unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"index": 0, "embedding": [0.1, 0.2, 0.3, 0.4]}]
            })))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(dir.path()).unwrap();
        let (_vectors, tier) = tier(&server, vec![seed("a", 2, Some(3))]).await;
        let svc = SearchService::open_with(pool, fts_idx, Some(tier));

        let out = svc
            .search(&SearchRequest {
                q: "different words entirely".into(),
                author_terms: Vec::new(),
                fields: fts::FieldSel::all(),
                keyword: true,
                semantic: true,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();

        assert_eq!(out.results.len(), 1);
        let m = &out.results[0].matched;
        assert_eq!(m.engine, Engine::Semantic);
        assert_eq!(m.field, fts::FieldName::Body);
        assert_eq!(m.page, Some(3));
        assert_eq!(
            m.snippet,
            Snippet::plain("novel <escaping> content"),
            "chunk text is shown as is, nothing marked"
        );
    }

    async fn degraded_reason(tier: SemanticTier) -> String {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("a", "Fuzzing Firmware"))
            .await
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(dir.path()).unwrap();
        fts_idx
            .upsert(&fts::PaperDoc {
                id: "a".into(),
                title: "Fuzzing Firmware".into(),
                authors: String::new(),
                venue: String::new(),
                abstract_text: String::new(),
                body: String::new(),
                notes: String::new(),
            })
            .unwrap();
        let svc = SearchService::open_with(pool, fts_idx, Some(tier));

        let out = svc
            .search(&SearchRequest {
                q: "fuzzing".into(),
                author_terms: Vec::new(),
                fields: fts::FieldSel::all(),
                keyword: true,
                semantic: true,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();

        assert!(!out.semantic.available);
        assert_eq!(out.results.len(), 1, "keyword results still returned");
        out.semantic.reason.expect("a degraded tier says why")
    }

    #[tokio::test]
    async fn semantic_failure_degrades_with_reason() {
        let (_vectors, tier) = crate::testutil::tier("http://127.0.0.1:1/v1", Vec::new()).await;
        degraded_reason(tier).await;
    }

    #[tokio::test]
    async fn a_failing_vector_store_degrades_semantic_but_keeps_keyword_hits() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"index": 0, "embedding": [0.1, 0.2, 0.3, 0.4]}]
            })))
            .mount(&server)
            .await;
        let (_vectors, tier) = tier(&server, Vec::new()).await;
        tier.store.close().await.unwrap();

        let reason = degraded_reason(tier).await;
        assert!(reason.contains("closed"), "got: {reason}");
    }

    #[tokio::test]
    async fn an_unavailable_vector_store_is_the_reported_reason() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = SearchConfig {
            index_dir: dir.path().join("index"),
            vector_dir: dir.path().join("vectors"),
        };
        let semantic = Semantic::Unavailable(vector::OpenError::Locked {
            dir: cfg.vector_dir.clone(),
            holder: None,
        });
        let (_db, pool) = crate::testutil::temp_pool().await;
        let svc = SearchService::open(pool, &cfg, semantic).await.unwrap();

        let st = svc.status().await.unwrap();
        assert!(!st.semantic_available);
        let reason = st.reason.unwrap();
        assert!(
            reason.contains("in use by another xuewen process"),
            "got: {reason}"
        );
    }

    #[tokio::test]
    async fn authors_only_selection_skips_semantic() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        let (_index, svc) = keyword_only_service(pool).await;
        let out = svc
            .search(&SearchRequest {
                q: "lovelace".into(),
                author_terms: Vec::new(),
                fields: fts::FieldSel {
                    authors: true,
                    ..fts::FieldSel::none()
                },
                keyword: true,
                semantic: true,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();
        // Semantic was requested but is meaningless for authors-only.
        assert!(!out.semantic.available);
    }

    #[tokio::test]
    async fn status_counts_pending_and_failed() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("a", "T"))
            .await
            .unwrap();
        let (_index, svc) = keyword_only_service(pool).await;
        let st = svc.status().await.unwrap();
        assert_eq!(st.fts.pending, 1); // never indexed
        assert_eq!(st.fts.failed, 0);
        assert!(!st.semantic_available);
    }

    #[tokio::test]
    async fn authors_only_disables_semantic_even_with_embedder() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("a", "Some Paper"))
            .await
            .unwrap();
        crate::search::store::replace_body(
            &pool,
            "a",
            &[crate::search::chunker::Chunk {
                seq: 1,
                page: Some(1),
                text: "Ada content".into(),
            }],
            "hash-a",
        )
        .await
        .unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"index": 0, "embedding": [0.1, 0.2, 0.3, 0.4]}]
            })))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(dir.path()).unwrap();
        let (_vectors, tier) = tier(&server, Vec::new()).await;
        let svc = SearchService::open_with(pool, fts_idx, Some(tier));

        let out = svc
            .search(&SearchRequest {
                q: "ada".into(),
                author_terms: Vec::new(),
                fields: fts::FieldSel {
                    authors: true,
                    ..fts::FieldSel::none()
                },
                keyword: true,
                semantic: true,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();

        assert!(
            !out.semantic.available,
            "semantic should be disabled for authors-only"
        );
        assert_eq!(
            out.semantic.reason.as_deref(),
            Some("semantic search does not apply to a query scoped to authors or notes"),
            "reason should explain the field selection disables semantic"
        );
    }

    #[tokio::test]
    async fn notes_only_selection_skips_semantic() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        let (_index, svc) = keyword_only_service(pool).await;
        let out = svc
            .search(&SearchRequest {
                q: "baseline".into(),
                author_terms: Vec::new(),
                fields: fts::FieldSel {
                    notes: true,
                    ..fts::FieldSel::none()
                },
                keyword: true,
                semantic: true,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();
        // Notes are indexed for keyword search only — nothing embeds them.
        assert!(!out.semantic.available);
    }

    #[tokio::test]
    async fn status_counts_failed_rows() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("a", "T"))
            .await
            .unwrap();
        crate::search::store::replace_body(
            &pool,
            "a",
            &[crate::search::chunker::Chunk {
                seq: 1,
                page: Some(1),
                text: "some chunk".into(),
            }],
            "hash-a",
        )
        .await
        .unwrap();
        crate::search::store::record_error(&pool, "a", "boom", store::ErrorTier::Both)
            .await
            .unwrap();

        let (_index, svc) = keyword_only_service(pool).await;
        let st = svc.status().await.unwrap();
        assert_eq!(st.fts.failed, 1, "status should count papers with errors");
    }

    #[tokio::test]
    async fn status_does_not_count_errors_on_tiers_with_no_pending_work() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        let p = paper("a", "T");
        crate::library::papers::insert(&pool, &p).await.unwrap();
        crate::search::store::replace_body(
            &pool,
            "a",
            &[crate::search::chunker::Chunk {
                seq: 1,
                page: Some(1),
                text: "T".into(),
            }],
            "hash-a",
        )
        .await
        .unwrap();
        // A pre-split failure once charged the vector tier too; keyword-only,
        // that tier never runs, so nothing ever clears its error...
        crate::search::store::record_error(&pool, "a", "boom", store::ErrorTier::Both)
            .await
            .unwrap();
        // ...and the next FTS pass succeeds.
        crate::search::store::mark_fts_done(&pool, &[("a", &store::doc_hash(&p, ""))])
            .await
            .unwrap();
        // Migration-0023-style residue: an error stamped on a tier that is
        // already current, which no pass will ever run to clear.
        sqlx::query("UPDATE search_index SET fts_last_error = 'legacy' WHERE paper_id = 'a'")
            .execute(&pool)
            .await
            .unwrap();

        let (_index, svc) = keyword_only_service(pool).await;
        let st = svc.status().await.unwrap();
        assert_eq!(st.fts.indexed, 1);
        assert_eq!(st.fts.pending, 0);
        assert_eq!(
            st.fts.failed, 0,
            "an error on a current tier is residue, not a failure"
        );
        assert_eq!(
            st.vectors.failed, 0,
            "no embedder: no vector pass exists to clear this error"
        );
    }

    #[tokio::test]
    async fn semantic_snippet_truncates_long_chunks_with_ellipsis() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("a", "Some Paper"))
            .await
            .unwrap();
        let long_text = "x".repeat(250);
        crate::search::store::replace_body(
            &pool,
            "a",
            &[crate::search::chunker::Chunk {
                seq: 2,
                page: Some(3),
                text: long_text,
            }],
            "hash-a",
        )
        .await
        .unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"index": 0, "embedding": [0.1, 0.2, 0.3, 0.4]}]
            })))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(dir.path()).unwrap();
        let (_vectors, tier) = tier(&server, vec![seed("a", 2, Some(3))]).await;
        let svc = SearchService::open_with(pool, fts_idx, Some(tier));

        let out = svc
            .search(&SearchRequest {
                q: "different words entirely".into(),
                author_terms: Vec::new(),
                fields: fts::FieldSel::all(),
                keyword: true,
                semantic: true,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();

        assert_eq!(out.results.len(), 1);
        let m = &out.results[0].matched;
        assert_eq!(m.engine, Engine::Semantic);
        let text = m.snippet.text();
        assert!(
            text.ends_with("…"),
            "snippet should end with ellipsis: {text}"
        );
        let text_before_ellipsis = text.trim_end_matches('…');
        assert_eq!(
            text_before_ellipsis.chars().count(),
            200,
            "text before ellipsis should be exactly 200 chars"
        );
    }

    #[tokio::test]
    async fn a_semantic_hit_without_its_chunk_is_dropped() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        for id in ["a", "b"] {
            crate::library::papers::insert(&pool, &paper(id, "Some Paper"))
                .await
                .unwrap();
        }
        // Only "a" still has the chunk its point was embedded from.
        crate::search::store::replace_body(
            &pool,
            "a",
            &[crate::search::chunker::Chunk {
                seq: 1,
                page: Some(1),
                text: "kept".into(),
            }],
            "hash-a",
        )
        .await
        .unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"index": 0, "embedding": [0.1, 0.2, 0.3, 0.4]}]
            })))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(dir.path()).unwrap();
        let (_vectors, tier) =
            tier(&server, vec![seed("a", 1, Some(1)), seed("b", 1, Some(1))]).await;
        let svc = SearchService::open_with(pool, fts_idx, Some(tier));

        let out = svc
            .search(&SearchRequest {
                q: "different words entirely".into(),
                author_terms: Vec::new(),
                fields: fts::FieldSel::all(),
                keyword: true,
                semantic: true,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();
        let ids: Vec<&str> = out.results.iter().map(|h| h.paper.id.as_str()).collect();
        assert_eq!(ids, ["a"]);
        assert_eq!(out.results[0].matched.snippet.text(), "kept");
    }

    #[tokio::test]
    async fn author_terms_scope_only_that_term_to_authors() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        for (id, title) in [("a", "Fuzzing Firmware"), ("b", "Sorting Networks")] {
            crate::library::papers::insert(&pool, &paper(id, title))
                .await
                .unwrap();
        }
        let (_index, svc) = keyword_only_service(pool).await;
        // "a" by Lovelace mentions smith in the body; "b" is BY smith.
        svc.fts
            .upsert(&fts::PaperDoc {
                id: "a".into(),
                title: "Fuzzing Firmware".into(),
                authors: "Ada Lovelace".into(),
                venue: String::new(),
                abstract_text: String::new(),
                body: "thanks to smith for fuzzing help".into(),
                notes: String::new(),
            })
            .unwrap();
        svc.fts
            .upsert(&fts::PaperDoc {
                id: "b".into(),
                title: "Sorting Networks".into(),
                authors: "Jane Smith".into(),
                venue: String::new(),
                abstract_text: String::new(),
                body: "batcher merge".into(),
                notes: String::new(),
            })
            .unwrap();

        let out = svc
            .search(&SearchRequest {
                q: String::new(),
                author_terms: vec!["smith".into()],
                fields: fts::FieldSel::all(),
                keyword: true,
                semantic: false,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();
        assert_eq!(
            out.results.len(),
            1,
            "smith-in-body must not match author:smith"
        );
        assert_eq!(out.results[0].paper.id, "b");
    }

    #[tokio::test]
    async fn author_terms_without_text_degrade_semantic_with_reason() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("a", "T"))
            .await
            .unwrap();
        let server = MockServer::start().await;
        // No embeddings mock: reaching the embedder would 404 loudly.
        let dir = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(dir.path()).unwrap();
        let (_vectors, tier) = tier(&server, Vec::new()).await;
        let svc = SearchService::open_with(pool, fts_idx, Some(tier));

        let out = svc
            .search(&SearchRequest {
                q: String::new(),
                author_terms: vec!["smith".into()],
                fields: fts::FieldSel::all(),
                keyword: true,
                semantic: true,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();
        assert!(!out.semantic.available);
        assert_eq!(
            out.semantic.reason.as_deref(),
            Some("semantic search does not apply to an authors-only query")
        );
    }

    #[tokio::test]
    async fn status_counts_backed_off_failures_as_pending() {
        let (_db, pool) = crate::testutil::temp_pool().await;
        crate::library::papers::insert(&pool, &paper("a", "T"))
            .await
            .unwrap();
        crate::search::store::replace_body(
            &pool,
            "a",
            &[crate::search::chunker::Chunk {
                seq: 1,
                page: Some(1),
                text: "T".into(),
            }],
            "hash-a",
        )
        .await
        .unwrap();
        crate::search::store::record_error(&pool, "a", "boom", store::ErrorTier::Both)
            .await
            .unwrap();
        let (_index, svc) = keyword_only_service(pool).await;
        let st = svc.status().await.unwrap();
        assert_eq!(
            st.fts.pending, 1,
            "failed+backed-off paper is still pending"
        );
        assert_eq!(st.fts.indexed, 0);
        assert_eq!(st.fts.failed, 1);
        assert_eq!(st.vectors.indexed, 0);
        assert_eq!(st.vectors.pending, 0, "no embedder -> vectors tier idle");
    }
}
