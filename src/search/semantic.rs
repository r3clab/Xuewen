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
        let Some((embedder, dims)) = embedder(ai) else {
            return Ok(Self::NotConfigured);
        };
        match VectorStore::open(&cfg.vector_dir, dims, mode).await {
            Ok((store, opened)) => {
                clear_stamps_if_empty(pool, opened).await?;
                Ok(Self::Ready(SemanticTier { embedder, store }))
            }
            Err(err) => {
                tracing::warn!("semantic search disabled: {err}");
                Ok(Self::Unavailable(err))
            }
        }
    }

    /// `open` for a long-running process: a store another process holds (a
    /// CLI command's catch-up sweep can outlast any fixed wait) is taken
    /// over by `acquire_waiting` once released, rather than leaving semantic
    /// search off until a restart.
    pub(crate) async fn open_deferred(
        pool: &SqlitePool,
        cfg: &SearchConfig,
        ai: &AiConfig,
    ) -> Result<Self> {
        let Some((embedder, dims)) = embedder(ai) else {
            return Ok(Self::NotConfigured);
        };
        match VectorStore::open(&cfg.vector_dir, dims, OpenMode::LoadOrCreate).await {
            Ok((store, opened)) => {
                clear_stamps_if_empty(pool, opened).await?;
                Ok(Self::Ready(SemanticTier { embedder, store }))
            }
            Err(err @ OpenError::Locked { .. }) => {
                tracing::warn!("semantic search waits: {err}");
                Ok(Self::Ready(SemanticTier {
                    embedder,
                    store: VectorStore::waiting(&cfg.vector_dir, dims),
                }))
            }
            Err(err) => {
                tracing::warn!("semantic search disabled: {err}");
                Ok(Self::Unavailable(err))
            }
        }
    }

    /// Try once to take over a store `open_deferred` found held; `true`
    /// while it is still waiting for its holder.
    pub(crate) async fn acquire_waiting(&self, pool: &SqlitePool) -> bool {
        let Some(store) = self.store().filter(|s| s.is_waiting()) else {
            return false;
        };
        match store.acquire().await {
            Ok(None) => true,
            Ok(Some(opened)) => {
                tracing::info!("semantic search started: the vector store was released");
                if let Err(e) = clear_stamps_if_empty(pool, opened).await {
                    tracing::warn!("could not schedule re-embedding the library: {e:#}");
                }
                false
            }
            Err(e) => {
                tracing::warn!("semantic search disabled: {e:#}");
                false
            }
        }
    }

    /// Only while its store is open.
    pub(crate) fn tier(&self) -> Option<&SemanticTier> {
        match self {
            Self::Ready(tier) if tier.store.unavailable().is_none() => Some(tier),
            Self::Ready(_) | Self::NotConfigured | Self::Unavailable(_) => None,
        }
    }

    /// The store whatever its state, for the consumers that share it.
    pub(crate) fn store(&self) -> Option<&VectorStore> {
        match self {
            Self::Ready(tier) => Some(&tier.store),
            Self::NotConfigured | Self::Unavailable(_) => None,
        }
    }

    pub(crate) fn holder(&self) -> Option<SocketAddr> {
        match self {
            Self::Unavailable(OpenError::Locked { holder, .. }) => *holder,
            _ => None,
        }
    }

    pub(crate) fn reason(&self) -> Option<String> {
        match self {
            Self::Ready(tier) => tier.store.unavailable(),
            Self::NotConfigured => {
                Some("embedding API not configured (set [ai.embedding] and an API key)".into())
            }
            Self::Unavailable(err) => Some(err.to_string()),
        }
    }
}

fn embedder(ai: &AiConfig) -> Option<(Embedder, usize)> {
    let e = ai.embedding.as_ref()?;
    let resolved = ai.resolve(&e.endpoint);
    Some((
        Embedder::from_resolved(&resolved, &e.model(), e.dims)?,
        e.dims,
    ))
}

/// An empty store means every vector stamp in SQLite is stale.
async fn clear_stamps_if_empty(pool: &SqlitePool, opened: Opened) -> Result<()> {
    if opened == Opened::Empty {
        tracing::info!("the vector store is empty; the library will be re-embedded");
        store::clear_stamps(pool, false, true).await?;
    }
    Ok(())
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
        store::replace_body(pool, "p1", &[], "h1").await.unwrap();
        store::mark_fts_done(pool, &[("p1", "dh")]).await.unwrap();
        mark_embedded(pool).await;
    }

    async fn mark_embedded(pool: &SqlitePool) {
        store::mark_vectors_done(pool, "p1", None, store::VectorsDone::All { model: "m1" })
            .await
            .unwrap();
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
    async fn a_deferred_open_takes_the_store_over_once_its_holder_lets_go() {
        let (_db, pool) = temp_pool().await;
        let dir = tempfile::tempdir().unwrap();
        let (cfg, ai) = (
            search_cfg(dir.path()),
            embedding_ai("http://127.0.0.1:1/v1", 4),
        );
        indexed_paper(&pool).await;
        let (holder, _) = VectorStore::open(&cfg.vector_dir, 4, OpenMode::LoadOrCreate)
            .await
            .unwrap();

        let semantic = Semantic::open_deferred(&pool, &cfg, &ai).await.unwrap();
        assert!(semantic.tier().is_none());
        let reason = semantic.reason().unwrap();
        assert!(reason.contains("in use"), "got: {reason}");
        assert!(semantic.store().is_some(), "shared with daily and the app");
        assert!(semantic.acquire_waiting(&pool).await, "still held");
        assert_eq!(stamps(&pool).await, (true, true));

        holder.close().await.unwrap();
        assert!(!semantic.acquire_waiting(&pool).await);
        assert!(semantic.tier().is_some() && semantic.reason().is_none());
        assert_eq!(
            stamps(&pool).await,
            (true, false),
            "an empty store re-embeds"
        );
        assert!(
            !semantic.acquire_waiting(&pool).await,
            "nothing left to wait for"
        );
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

        mark_embedded(&pool).await;
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
