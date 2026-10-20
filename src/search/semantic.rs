use std::net::SocketAddr;

use anyhow::Result;
use sqlx::SqlitePool;

use super::embedder::Embedder;
use super::store;
use super::vector::{OpenError, OpenMode, Opened, VectorStore};
use crate::config::{AiConfig, SearchConfig};

/// The semantic tier's two halves. They live and die together — a vector
/// store is useless without an embedder to query it — so the pairing is one
/// value instead of two separately-optional ones re-guarded at every call site.
pub struct SemanticTier {
    pub(crate) embedder: Embedder,
    pub(crate) store: VectorStore,
}

/// A vector store that fails to open is a state, not an error: it disables
/// this tier only, never keyword search or the app.
pub(crate) enum Semantic {
    NotConfigured,
    Unavailable(OpenError),
    Ready(SemanticTier),
}

impl Semantic {
    /// `Err` only when SQLite fails.
    pub(crate) async fn open(
        pool: &SqlitePool,
        cfg: &SearchConfig,
        ai: &AiConfig,
        mode: OpenMode,
    ) -> Result<Self> {
        let Some(e) = &ai.embedding else {
            return Ok(Self::NotConfigured);
        };
        let resolved = ai.resolve(&e.endpoint);
        let Some(embedder) = Embedder::from_resolved(&resolved, &e.model(), e.dims) else {
            return Ok(Self::NotConfigured);
        };
        match VectorStore::open(&cfg.vector_dir, e.dims, mode).await {
            Ok((store, opened)) => {
                if opened == Opened::Empty {
                    tracing::info!(
                        "vector store at {} is empty; the library will be re-embedded",
                        cfg.vector_dir.display()
                    );
                    store::clear_stamps(pool, false, true).await?;
                }
                Ok(Self::Ready(SemanticTier { embedder, store }))
            }
            Err(err) => {
                tracing::warn!("semantic search disabled: {err}");
                Ok(Self::Unavailable(err))
            }
        }
    }

    pub(crate) fn tier(&self) -> Option<&SemanticTier> {
        match self {
            Self::Ready(tier) => Some(tier),
            Self::NotConfigured | Self::Unavailable(_) => None,
        }
    }

    pub(crate) fn store(&self) -> Option<&VectorStore> {
        self.tier().map(|t| &t.store)
    }

    pub(crate) fn holder(&self) -> Option<SocketAddr> {
        match self {
            Self::Unavailable(OpenError::Locked { holder, .. }) => *holder,
            _ => None,
        }
    }

    pub(crate) fn reason(&self) -> Option<String> {
        match self {
            Self::Ready(_) => None,
            Self::NotConfigured => {
                Some("embedding API not configured (set [ai.embedding] and an API key)".into())
            }
            Self::Unavailable(err) => Some(err.to_string()),
        }
    }
}

impl From<Option<SemanticTier>> for Semantic {
    fn from(tier: Option<SemanticTier>) -> Self {
        tier.map_or(Self::NotConfigured, Self::Ready)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{embedding_ai, sample_paper, temp_pool};

    async fn indexed_paper(pool: &SqlitePool) {
        crate::library::papers::insert(pool, &sample_paper("p1", "h1"))
            .await
            .unwrap();
        store::replace_chunks(pool, "p1", &[], "h1", "mh")
            .await
            .unwrap();
        store::mark_fts_done(pool, "p1", "nh").await.unwrap();
        store::mark_vectors_done(pool, "p1", "m1").await.unwrap();
    }

    async fn stamps(pool: &SqlitePool) -> (bool, bool) {
        let row = &store::all_index_rows(pool).await.unwrap()[0];
        (
            row.fts_indexed_at.is_some(),
            row.vectors_indexed_at.is_some(),
        )
    }

    fn search_cfg(dir: &std::path::Path) -> SearchConfig {
        SearchConfig {
            index_dir: dir.join("index"),
            vector_dir: dir.join("vectors"),
        }
    }

    #[tokio::test]
    async fn an_empty_store_clears_vector_stamps_and_a_populated_one_keeps_them() {
        let (_db, pool) = temp_pool().await;
        let dir = tempfile::tempdir().unwrap();
        let (cfg, ai) = (
            search_cfg(dir.path()),
            embedding_ai("http://127.0.0.1:1/v1", 4),
        );
        indexed_paper(&pool).await;

        let semantic = Semantic::open(&pool, &cfg, &ai, OpenMode::LoadOrCreate)
            .await
            .unwrap();
        assert!(semantic.tier().is_some());
        assert_eq!(stamps(&pool).await, (true, false), "only vectors re-embed");
        let store = semantic.store().unwrap();
        store
            .upsert(vec![crate::testutil::chunk("p1", 0, None, vec![1.0; 4])])
            .await
            .unwrap();
        store.close().await.unwrap();

        store::mark_vectors_done(&pool, "p1", "m1").await.unwrap();
        let semantic = Semantic::open(&pool, &cfg, &ai, OpenMode::LoadOrCreate)
            .await
            .unwrap();
        assert!(semantic.tier().is_some());
        assert_eq!(stamps(&pool).await, (true, true));
    }

    #[tokio::test]
    async fn a_held_store_is_unavailable_with_its_holder() {
        let (_db, pool) = temp_pool().await;
        let dir = tempfile::tempdir().unwrap();
        let (cfg, ai) = (
            search_cfg(dir.path()),
            embedding_ai("http://127.0.0.1:1/v1", 4),
        );
        let held = Semantic::open(&pool, &cfg, &ai, OpenMode::LoadOrCreate)
            .await
            .unwrap();
        let addr: SocketAddr = "127.0.0.1:8123".parse().unwrap();
        held.store().unwrap().publish_addr(addr).await.unwrap();

        let semantic = Semantic::open(&pool, &cfg, &ai, OpenMode::LoadOrCreate)
            .await
            .unwrap();
        assert!(semantic.tier().is_none());
        assert_eq!(semantic.holder(), Some(addr));
        let reason = semantic.reason().unwrap();
        assert!(reason.contains("in use"), "got: {reason}");
    }

    #[tokio::test]
    async fn without_an_embedder_no_store_is_created() {
        let (_db, pool) = temp_pool().await;
        let dir = tempfile::tempdir().unwrap();
        let cfg = search_cfg(dir.path());
        let semantic = Semantic::open(&pool, &cfg, &AiConfig::default(), OpenMode::LoadOrCreate)
            .await
            .unwrap();
        assert!(matches!(semantic, Semantic::NotConfigured));
        assert!(semantic.reason().unwrap().contains("not configured"));
        assert!(!cfg.vector_dir.exists());
    }
}
