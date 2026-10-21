//! Shared test fixtures. Compiled only for tests — `#[cfg(test)]` for the
//! unit tests, the `test-support` feature (which `tests/` turns on through a
//! self dev-dependency) for the integration tests — so nothing here can leak
//! into a release build.

use crate::library::model::{Authors, Paper, PaperMeta, PaperStatus};

pub async fn temp_pool() -> (tempfile::TempDir, sqlx::SqlitePool) {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = crate::db::connect(&url).await.unwrap();
    (dir, pool)
}

pub fn sample_paper(id: &str, hash: &str) -> Paper {
    Paper {
        id: id.to_string(),
        content_hash: hash.to_string(),
        rel_path: format!("{hash}.pdf"),
        cite_key: None,
        added_at: "2026-07-06T00:00:00Z".to_string(),
        deleted_at: None,
        starred: false,
        name: None,
        meta: PaperMeta {
            title: Some("A Title".into()),
            abstract_text: None,
            authors: Authors::default(),
            venue: None,
            year: None,
            doi: None,
            arxiv_id: None,
            dblp_key: None,
            url: None,
            source: None,
            status: PaperStatus::NeedsReview,
        },
    }
}

pub fn paper(id: &str, title: &str, status: PaperStatus) -> Paper {
    Paper {
        id: id.into(),
        content_hash: id.into(),
        rel_path: format!("{id}.pdf"),
        cite_key: Some(id.into()),
        added_at: "2026-07-07T00:00:00Z".into(),
        deleted_at: None,
        starred: false,
        name: None,
        meta: PaperMeta {
            title: Some(title.into()),
            abstract_text: Some("An abstract.".into()),
            authors: Authors(vec!["Ada Lovelace".into()]),
            venue: Some("KDD".into()),
            year: Some(2020),
            doi: None,
            arxiv_id: None,
            dblp_key: None,
            url: None,
            source: Some("crossref".into()),
            status,
        },
    }
}

/// Migrated pool with one seeded paper — the setup every subsystem whose
/// tables FK onto `papers(id)` (citations, annotations) needs. `rel_path`
/// points at a file that doesn't exist, so any extraction attempt (e.g.
/// chat's `paper_text`) fails and falls back to metadata alone. Returns the
/// `TempDir` so the caller's `let (pool, _dir)` keeps the database directory
/// alive for the test's duration and removes it afterwards. (Drop order —
/// dir before pool — is fine: SQLite on unix keeps working against an
/// unlinked file.)
pub async fn pool_with_paper(id: &str) -> (sqlx::SqlitePool, tempfile::TempDir) {
    let (dir, pool) = temp_pool().await;
    let mut seeded = paper(id, &format!("Paper {id}"), PaperStatus::Resolved);
    seeded.rel_path = "missing.pdf".into();
    crate::library::papers::insert(&pool, &seeded)
        .await
        .unwrap();
    (pool, dir)
}

/// Insert a minimal paper (mirroring `sample_paper`) and return its id.
pub async fn insert_test_paper(pool: &sqlx::SqlitePool) -> String {
    let id = uuid::Uuid::now_v7().to_string();
    let p = sample_paper(&id, &id);
    crate::library::papers::insert(pool, &p).await.unwrap();
    id
}

/// Leaks its temp dir, like `pool_with_paper`.
pub async fn temp_vectors(dims: usize) -> crate::search::vector::VectorStore {
    use crate::search::vector::{OpenMode, VectorStore};
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = VectorStore::open(&dir.path().join("vectors"), dims, OpenMode::LoadOrCreate)
        .await
        .unwrap();
    std::mem::forget(dir);
    store
}

#[cfg(test)]
pub(crate) fn chunk(
    paper: &str,
    seq: i64,
    page: Option<i64>,
    vector: Vec<f32>,
) -> crate::search::vector::ChunkPoint {
    crate::search::vector::ChunkPoint {
        paper_id: paper.into(),
        seq,
        page,
        vector,
    }
}

#[cfg(test)]
pub(crate) async fn tier(
    base_url: &str,
    seeds: Vec<crate::search::vector::ChunkPoint>,
) -> crate::search::SemanticTier {
    let store = temp_vectors(4).await;
    store.upsert(seeds).await.unwrap();
    crate::search::SemanticTier {
        embedder: crate::search::embedder::Embedder::for_tests(base_url, "m1", 4),
        store,
    }
}

pub fn embedding_ai(base_url: &str, dims: usize) -> crate::config::AiConfig {
    toml::from_str(&format!(
        "[embedding]\nbase_url = \"{base_url}\"\napi_key = \"k\"\nmodel = \"m1\"\ndims = {dims}\n"
    ))
    .unwrap()
}

#[cfg(test)]
pub(crate) async fn serve(app: crate::app::App) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(crate::web::serve_on(
        listener,
        app,
        tokio_util::sync::CancellationToken::new(),
    ));
    addr
}
