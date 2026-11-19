use std::fmt;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use qdrant_edge::{
    Condition, CreateIndex, Distance, EdgeConfig, EdgeShard, EdgeVectorParams, FieldCondition,
    FieldIndexOperations, Filter, JsonPath, Match, Memory, NamedQuery, OperationError, Payload,
    PayloadFieldSchema, PayloadSchemaType, PointId, PointInsertOperations, PointOperations,
    PointStruct, QueryEnum, Range, ScrollRequestBuilder, SearchRequestBuilder, UpdateOperation,
    VectorStructInternal, WithPayloadInterface, WithVector, DEFAULT_VECTOR_NAME,
};
use serde::Deserialize;
use serde_json::json;

const UPSERT_BATCH: usize = 64;
const SCROLL_PAGE: usize = 256;
const LOCK_FILE: &str = "xuewen.lock";
/// Written by `EdgeShard::new`; a directory without it holds no shard.
const SHARD_CONFIG_FILE: &str = "edge_config.json";
const CLOSE_GRACE: Duration = Duration::from_secs(5);

/// One chunk's embedding, ready for the vector store. Chunk text stays in SQLite.
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
fn point_id(paper_id: &str, seq: i64) -> PointId {
    PointId::Uuid(uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("{paper_id}:{seq}").as_bytes(),
    ))
}

#[derive(Deserialize)]
struct PointPayload {
    paper_id: String,
    seq: i64,
    /// NULL for the seq-0 title+abstract chunk.
    #[serde(default)]
    page: Option<i64>,
}

impl PointPayload {
    fn parse(payload: Option<Payload>) -> Result<Self> {
        let map = payload
            .ok_or_else(|| anyhow!("vector store point has no payload"))?
            .0;
        Ok(serde_json::from_value(serde_json::Value::Object(map))?)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    LoadOrCreate,
    Recreate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    /// No points, so every vector stamp in SQLite is stale. Judged by content
    /// rather than by whether this open created the store, so a crash before
    /// the stamps were cleared still re-embeds on the next start.
    Empty,
    Populated,
}

#[derive(Debug)]
pub enum OpenError {
    Locked {
        dir: PathBuf,
        /// Where the holder serves HTTP, when it is a server.
        holder: Option<SocketAddr>,
    },
    DimsMismatch {
        dir: PathBuf,
        stored: usize,
        configured: usize,
    },
    Unusable {
        dir: PathBuf,
        detail: String,
    },
    Io {
        dir: PathBuf,
        detail: String,
    },
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const REBUILD: &str = "run: xuewen index rebuild --vectors-only";
        match self {
            Self::Locked {
                dir,
                holder: Some(addr),
            } => write!(
                f,
                "vector store {} is in use by the xuewen server at http://{addr}",
                dir.display()
            ),
            Self::Locked { dir, holder: None } => write!(
                f,
                "vector store {} is in use by another xuewen process \
                 (is `xuewen watch` or `xuewen index rebuild` running?)",
                dir.display()
            ),
            Self::DimsMismatch {
                dir,
                stored,
                configured,
            } => write!(
                f,
                "vector store {} holds {stored}-dim vectors but [ai.embedding].dims = \
                 {configured} — {REBUILD}",
                dir.display()
            ),
            Self::Unusable { dir, detail } => {
                write!(
                    f,
                    "vector store {} is unusable ({detail}) — {REBUILD}",
                    dir.display()
                )
            }
            Self::Io { dir, detail } => write!(f, "vector store {}: {detail}", dir.display()),
        }
    }
}

impl std::error::Error for OpenError {}

impl OpenError {
    fn io(dir: &Path, e: impl fmt::Display) -> Self {
        Self::Io {
            dir: dir.to_path_buf(),
            detail: e.to_string(),
        }
    }

    fn unusable(dir: &Path, e: impl fmt::Display) -> Self {
        Self::Unusable {
            dir: dir.to_path_buf(),
            detail: e.to_string(),
        }
    }
}

/// Exclusive lock on the store directory, held for the store's lifetime. Our
/// own flock rather than edge's WAL lock: edge reports that one only as an
/// untyped message, and takes it too late to guard a `Recreate` wipe. While
/// held, the file carries the holder's HTTP address for CLI forwarding.
struct DirLock {
    file: File,
}

impl DirLock {
    fn acquire(dir: &Path) -> Result<Self, OpenError> {
        let io = |e| OpenError::io(dir, e);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(LOCK_FILE))
            .map_err(io)?;
        match file.try_lock() {
            Ok(()) => {
                file.set_len(0).map_err(io)?;
                Ok(Self { file })
            }
            Err(TryLockError::WouldBlock) => {
                let mut published = String::new();
                let holder = file
                    .read_to_string(&mut published)
                    .ok()
                    .and_then(|_| published.trim().parse().ok());
                Err(OpenError::Locked {
                    dir: dir.to_path_buf(),
                    holder,
                })
            }
            Err(TryLockError::Error(e)) => Err(OpenError::io(dir, e)),
        }
    }

    fn publish(&self, addr: SocketAddr) -> std::io::Result<()> {
        let mut file = &self.file;
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        writeln!(file, "{addr}")
    }
}

/// The embedded Qdrant Edge shard behind semantic search and the daily
/// profile. Clones share one shard, one lock, and one `close`.
#[derive(Clone)]
pub struct VectorStore {
    state: Arc<RwLock<State>>,
}

enum State {
    /// Another process holds the directory; every operation fails until
    /// `acquire` takes the lock over.
    Waiting {
        dir: PathBuf,
        dims: usize,
        /// Published once the lock is ours.
        publish: Option<SocketAddr>,
    },
    Open(Held),
    /// `acquire` found the store unusable.
    Failed(String),
    Closed,
}

impl State {
    fn unavailable(&self) -> Option<String> {
        match self {
            Self::Open(_) => None,
            Self::Waiting { dir, .. } => Some(format!(
                "vector store {} is in use by another xuewen process; semantic search \
                 starts once it is released",
                dir.display()
            )),
            Self::Failed(reason) => Some(reason.clone()),
            Self::Closed => Some("vector store is closed".into()),
        }
    }
}

struct Held {
    // Field order is drop order: the shard flushes before the lock goes.
    shard: EdgeShard,
    lock: DirLock,
}

impl VectorStore {
    pub async fn open(
        dir: &Path,
        dims: usize,
        mode: OpenMode,
    ) -> Result<(Self, Opened), OpenError> {
        let owned = dir.to_path_buf();
        let (held, opened) = tokio::task::spawn_blocking(move || open_blocking(&owned, dims, mode))
            .await
            .map_err(|e| OpenError::io(dir, format!("open task panicked: {e}")))??;
        Ok((Self::with_state(State::Open(held)), opened))
    }

    /// A store at `dir` whose lock another process holds, for `acquire` to
    /// take over once that process lets go.
    pub(crate) fn waiting(dir: &Path, dims: usize) -> Self {
        Self::with_state(State::Waiting {
            dir: dir.to_path_buf(),
            dims,
            publish: None,
        })
    }

    fn with_state(state: State) -> Self {
        Self {
            state: Arc::new(RwLock::new(state)),
        }
    }

    pub(crate) fn is_waiting(&self) -> bool {
        matches!(
            *self.state.read().unwrap_or_else(PoisonError::into_inner),
            State::Waiting { .. }
        )
    }

    /// Why operations fail right now; `None` while the store is open.
    pub(crate) fn unavailable(&self) -> Option<String> {
        self.state
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .unavailable()
    }

    /// Take over a `waiting` store's lock. `Ok(None)` while it is still held
    /// elsewhere (or the store is not waiting); an unusable store fails for
    /// good.
    pub(crate) async fn acquire(&self) -> Result<Option<Opened>> {
        let state = Arc::clone(&self.state);
        tokio::task::spawn_blocking(move || {
            let mut state = state.write().unwrap_or_else(PoisonError::into_inner);
            let State::Waiting { dir, dims, publish } = &*state else {
                return Ok(None);
            };
            match open_blocking(dir, *dims, OpenMode::LoadOrCreate) {
                Ok((held, opened)) => {
                    if let Some(addr) = publish {
                        if let Err(e) = held.lock.publish(*addr) {
                            tracing::warn!("could not publish the server address for the CLI: {e}");
                        }
                    }
                    *state = State::Open(held);
                    Ok(Some(opened))
                }
                Err(OpenError::Locked { .. }) => Ok(None),
                Err(e) => {
                    *state = State::Failed(e.to_string());
                    Err(e.into())
                }
            }
        })
        .await
        .context("vector store task panicked")?
    }

    /// Edge calls block on disk I/O; this runs them on the blocking pool so
    /// they can't stall the tokio workers serving HTTP.
    async fn run<T: Send + 'static>(
        &self,
        op: impl FnOnce(&Held) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let state = Arc::clone(&self.state);
        tokio::task::spawn_blocking(move || {
            match &*state.read().unwrap_or_else(PoisonError::into_inner) {
                State::Open(held) => op(held),
                other => Err(anyhow!(other.unavailable().unwrap_or_default())),
            }
        })
        .await
        .context("vector store task panicked")?
    }

    #[cfg(test)]
    pub(crate) async fn upsert(&self, points: Vec<ChunkPoint>) -> Result<()> {
        self.run(move |held| held.write_points(points)).await
    }

    /// Make `points` the paper's only points: a re-chunk can leave fewer
    /// chunks than the last one, and their points must not outlive them.
    pub(crate) async fn replace_paper(
        &self,
        paper_id: &str,
        points: Vec<ChunkPoint>,
    ) -> Result<()> {
        let filter = paper_filter(paper_id);
        self.run(move |held| {
            held.shard.update(UpdateOperation::PointOperation(
                PointOperations::DeletePointsByFilter(filter),
            ))?;
            held.write_points(points)
        })
        .await
    }

    /// Replace the paper's seq-0 point, or remove it for a paper that no
    /// longer has a title or abstract.
    pub(crate) async fn replace_summary(
        &self,
        paper_id: &str,
        point: Option<ChunkPoint>,
    ) -> Result<()> {
        let id = point_id(paper_id, 0);
        self.run(move |held| match point {
            Some(point) => held.write_points(vec![point]),
            None => {
                held.shard.update(UpdateOperation::PointOperation(
                    PointOperations::DeletePoints { ids: vec![id] },
                ))?;
                Ok(held.shard.flush()?)
            }
        })
        .await
    }

    /// Vector search. `scope`, when given, restricts hits to those paper ids
    /// INSIDE the store (a `match any` clause), so the top-`limit` truncation
    /// happens on the filtered set.
    pub(crate) async fn search(
        &self,
        vector: &[f32],
        limit: usize,
        filter: SeqFilter,
        scope: Option<&[String]>,
    ) -> Result<Vec<VecHit>> {
        let mut must = Vec::new();
        match filter {
            SeqFilter::All => {}
            SeqFilter::OnlySummary => must.push(is_summary()),
            SeqFilter::OnlyBody => must.push(Condition::Field(FieldCondition::new_range(
                key("seq"),
                Range {
                    gte: Some(1.0.into()),
                    ..Default::default()
                },
            ))),
        }
        if let Some(ids) = scope {
            must.push(Condition::Field(FieldCondition::new_match(
                key("paper_id"),
                Match::from(ids.to_vec()),
            )));
        }
        let mut request = SearchRequestBuilder::new(
            QueryEnum::Nearest(NamedQuery {
                query: vector.to_vec().into(),
                using: None,
            }),
            limit,
        )
        .with_payload(WithPayloadInterface::Bool(true));
        if !must.is_empty() {
            request = request.filter(Filter {
                must: Some(must),
                ..Default::default()
            });
        }
        let request = request.build();
        self.run(move |held| {
            held.shard
                .search(request)?
                .into_iter()
                .map(|p| {
                    let payload = PointPayload::parse(p.payload)?;
                    Ok(VecHit {
                        paper_id: payload.paper_id,
                        seq: payload.seq,
                        page: payload.page,
                    })
                })
                .collect()
        })
        .await
    }

    /// All seq-0 (title+abstract) points as (paper_id, vector). Feeds the
    /// daily-recommendation profile. Cosine storage hands vectors back
    /// L2-normalized, not as upserted.
    pub(crate) async fn scroll_summaries(&self) -> Result<Vec<(String, Vec<f32>)>> {
        self.run(|held| {
            let mut out = Vec::new();
            let mut offset = None;
            loop {
                let mut request = ScrollRequestBuilder::new()
                    .limit(SCROLL_PAGE)
                    .filter(Filter::new_must(is_summary()))
                    .with_payload(WithPayloadInterface::Bool(true))
                    .with_vector(WithVector::Bool(true));
                if let Some(o) = offset {
                    request = request.offset(o);
                }
                let (records, next) = held.shard.scroll(request.build())?;
                for r in records {
                    let payload = PointPayload::parse(r.payload)?;
                    let Some(VectorStructInternal::Single(v)) = r.vector else {
                        bail!("vector store point {} has no dense vector", r.id);
                    };
                    out.push((payload.paper_id, v));
                }
                offset = next;
                if offset.is_none() {
                    return Ok(out);
                }
            }
        })
        .await
    }

    pub(crate) async fn delete_paper(&self, paper_id: &str) -> Result<()> {
        let filter = paper_filter(paper_id);
        self.run(move |held| {
            held.shard.update(UpdateOperation::PointOperation(
                PointOperations::DeletePointsByFilter(filter),
            ))?;
            Ok(held.shard.flush()?)
        })
        .await
    }

    /// Edge has no background optimizer: segments only merge and get
    /// HNSW-indexed when asked, and this blocks for as long as that takes.
    pub(crate) async fn optimize(&self) -> Result<bool> {
        self.run(|held| Ok(held.shard.optimize()?)).await
    }

    /// A waiting store publishes `addr` once it takes the lock over.
    pub(crate) async fn publish_addr(&self, addr: SocketAddr) -> Result<()> {
        let addr = reachable(addr);
        let state = Arc::clone(&self.state);
        tokio::task::spawn_blocking(move || {
            match &mut *state.write().unwrap_or_else(PoisonError::into_inner) {
                State::Open(held) => Ok(held.lock.publish(addr)?),
                State::Waiting { publish, .. } => {
                    *publish = Some(addr);
                    Ok(())
                }
                other => Err(anyhow!(other.unavailable().unwrap_or_default())),
            }
        })
        .await
        .context("vector store task panicked")?
    }

    /// Flush and release the lock even while clones remain; they fail from
    /// then on.
    pub async fn close(&self) -> Result<()> {
        let state = Arc::clone(&self.state);
        tokio::task::spawn_blocking(move || {
            let mut state = state.write().unwrap_or_else(PoisonError::into_inner);
            match std::mem::replace(&mut *state, State::Closed) {
                State::Open(held) => Ok(held.shard.flush()?),
                _ => Ok(()),
            }
        })
        .await
        .context("vector store task panicked")?
    }

    /// Every update flushes before it returns, so a close that cannot finish
    /// (an optimize still running) is safe to abandon at exit.
    pub(crate) async fn close_gracefully(&self) {
        match tokio::time::timeout(CLOSE_GRACE, self.close()).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!("closing vector store: {e:#}"),
            Err(_) => tracing::warn!(
                "vector store still busy after {}s; exiting without closing it",
                CLOSE_GRACE.as_secs()
            ),
        }
    }
}

fn open_blocking(dir: &Path, dims: usize, mode: OpenMode) -> Result<(Held, Opened), OpenError> {
    let io = |e| OpenError::io(dir, e);
    let unusable = |e| OpenError::unusable(dir, e);
    std::fs::create_dir_all(dir).map_err(io)?;
    let lock = DirLock::acquire(dir)?;
    if mode == OpenMode::Recreate {
        clear_except_lock(dir).map_err(io)?;
    }
    let shard = if dir.join(SHARD_CONFIG_FILE).exists() {
        let shard = EdgeShard::load(dir, None).map_err(unusable)?;
        let stored = shard
            .config()
            .vectors
            .get(DEFAULT_VECTOR_NAME)
            .map(|p| p.size);
        match stored {
            Some(stored) if stored == dims => {}
            Some(stored) => {
                return Err(OpenError::DimsMismatch {
                    dir: dir.to_path_buf(),
                    stored,
                    configured: dims,
                })
            }
            None => return Err(OpenError::unusable(dir, "no unnamed vector configured")),
        }
        shard
    } else {
        EdgeShard::new(dir, edge_config(dims)).map_err(unusable)?
    };
    ensure_payload_indexes(&shard).map_err(unusable)?;
    let opened = match shard.info().map_err(unusable)?.points_count {
        0 => Opened::Empty,
        _ => Opened::Populated,
    };
    Ok((Held { shard, lock }, opened))
}

impl Held {
    fn write_points(&self, points: Vec<ChunkPoint>) -> Result<()> {
        let mut points = points.into_iter().map(|p| {
            PointStruct::new(
                point_id(&p.paper_id, p.seq),
                p.vector,
                json!({"paper_id": p.paper_id, "seq": p.seq, "page": p.page}),
            )
            .into()
        });
        loop {
            let batch: Vec<_> = points.by_ref().take(UPSERT_BATCH).collect();
            if batch.is_empty() {
                return Ok(self.shard.flush()?);
            }
            self.shard.update(UpdateOperation::PointOperation(
                PointOperations::UpsertPoints(PointInsertOperations::PointsList(batch)),
            ))?;
        }
    }
}

fn paper_filter(paper_id: &str) -> Filter {
    Filter::new_must(Condition::Field(FieldCondition::new_match(
        key("paper_id"),
        Match::from(paper_id.to_string()),
    )))
}

fn edge_config(dims: usize) -> EdgeConfig {
    EdgeConfig::builder()
        .vector(
            DEFAULT_VECTOR_NAME,
            EdgeVectorParams::builder(dims, Distance::Cosine)
                .memory(Memory::Cold)
                .build(),
        )
        .build()
}

/// Checked on every open rather than only at creation, so a crash between
/// `EdgeShard::new` and the index creation heals itself.
fn ensure_payload_indexes(shard: &EdgeShard) -> Result<(), OperationError> {
    let schema = shard.info()?.payload_schema;
    for (field, kind) in [
        ("paper_id", PayloadSchemaType::Keyword),
        ("seq", PayloadSchemaType::Integer),
    ] {
        let field = key(field);
        if !schema.contains_key(&field) {
            shard.update(UpdateOperation::FieldIndexOperation(
                FieldIndexOperations::CreateIndex(CreateIndex {
                    field_name: field,
                    field_schema: Some(PayloadFieldSchema::FieldType(kind)),
                }),
            ))?;
        }
    }
    Ok(())
}

fn clear_except_lock(dir: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name() == LOCK_FILE {
            continue;
        }
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// A server bound to the unspecified address (containers bind 0.0.0.0) is
/// reachable on loopback, which is what a local CLI dials.
fn reachable(mut addr: SocketAddr) -> SocketAddr {
    if addr.ip().is_unspecified() {
        addr.set_ip(match addr.ip() {
            IpAddr::V4(_) => Ipv4Addr::LOCALHOST.into(),
            IpAddr::V6(_) => Ipv6Addr::LOCALHOST.into(),
        });
    }
    addr
}

fn key(path: &str) -> JsonPath {
    path.parse().expect("static payload key")
}

fn is_summary() -> Condition {
    Condition::Field(FieldCondition::new_match(key("seq"), Match::from(0i64)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{chunk as point, temp_vectors};

    fn hit_ids(hits: &[VecHit]) -> Vec<(String, i64)> {
        hits.iter().map(|h| (h.paper_id.clone(), h.seq)).collect()
    }

    async fn open(dir: &Path, dims: usize) -> Result<(VectorStore, Opened), OpenError> {
        VectorStore::open(dir, dims, OpenMode::LoadOrCreate).await
    }

    #[test]
    fn point_ids_are_deterministic_uuids() {
        let a = point_id("p1", 0);
        assert_eq!(a, point_id("p1", 0));
        assert_ne!(a, point_id("p1", 1));
        assert_ne!(a, point_id("p2", 0));
        assert!(matches!(a, PointId::Uuid(_)));
    }

    #[tokio::test]
    async fn a_store_opens_as_empty_until_it_holds_a_point() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("vectors");
        let (store, opened) = open(&dir, 4).await.unwrap();
        assert_eq!(opened, Opened::Empty);
        store.close().await.unwrap();

        let (store, opened) = open(&dir, 4).await.unwrap();
        assert_eq!(opened, Opened::Empty, "created but never filled");
        store
            .upsert(vec![point("p1", 0, None, vec![1.0, 0.0, 0.0, 0.0])])
            .await
            .unwrap();
        store.close().await.unwrap();

        let (_store, opened) = open(&dir, 4).await.unwrap();
        assert_eq!(opened, Opened::Populated);
    }

    /// A copy of the directory is what a SIGKILL leaves behind: edge never
    /// replays its WAL on load.
    async fn reopened_copy(dir: &Path) -> (tempfile::TempDir, VectorStore) {
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for entry in std::fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                let target = to.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    copy(&entry.path(), &target);
                } else {
                    std::fs::copy(entry.path(), target).unwrap();
                }
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        copy(dir, tmp.path());
        let (store, _) = open(tmp.path(), 4).await.unwrap();
        (tmp, store)
    }

    #[tokio::test]
    async fn upserts_and_deletes_are_on_disk_when_they_return() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("vectors");
        let (store, _) = open(&dir, 4).await.unwrap();
        let q = [1.0, 1.0, 0.0, 0.0];
        store
            .upsert(vec![
                point("p1", 0, None, vec![1.0, 0.0, 0.0, 0.0]),
                point("p2", 0, None, vec![0.0, 1.0, 0.0, 0.0]),
            ])
            .await
            .unwrap();
        let (_copy, crashed) = reopened_copy(&dir).await;
        let hits = crashed.search(&q, 10, SeqFilter::All, None).await.unwrap();
        assert_eq!(hits.len(), 2, "got: {:?}", hit_ids(&hits));

        store.delete_paper("p2").await.unwrap();
        let (_copy, crashed) = reopened_copy(&dir).await;
        let hits = crashed.search(&q, 10, SeqFilter::All, None).await.unwrap();
        assert_eq!(hit_ids(&hits), [("p1".to_string(), 0)]);
    }

    #[tokio::test]
    async fn data_survives_close_and_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("vectors");
        let (store, _) = open(&dir, 4).await.unwrap();
        store
            .upsert(vec![point("p1", 2, Some(5), vec![1.0, 0.0, 0.0, 0.0])])
            .await
            .unwrap();
        store.close().await.unwrap();

        let (store, _) = open(&dir, 4).await.unwrap();
        let hits = store
            .search(&[1.0, 0.0, 0.0, 0.0], 10, SeqFilter::All, None)
            .await
            .unwrap();
        assert_eq!(hit_ids(&hits), [("p1".to_string(), 2)]);
        assert_eq!(hits[0].page, Some(5));
    }

    #[tokio::test]
    async fn reopening_with_other_dims_reports_mismatch_with_the_rebuild_hint() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("vectors");
        open(&dir, 8).await.unwrap().0.close().await.unwrap();
        let err = open(&dir, 4).await.err().unwrap();
        assert!(
            matches!(
                err,
                OpenError::DimsMismatch {
                    stored: 8,
                    configured: 4,
                    ..
                }
            ),
            "got: {err}"
        );
        assert!(
            err.to_string().contains("rebuild --vectors-only"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn second_open_of_a_held_directory_is_locked_even_in_the_same_process() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("vectors");
        let (_held, _) = open(&dir, 4).await.unwrap();
        let err = open(&dir, 4).await.err().unwrap();
        assert!(
            matches!(err, OpenError::Locked { holder: None, .. }),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn locked_error_carries_the_holders_published_address_on_loopback() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("vectors");
        let (held, _) = open(&dir, 4).await.unwrap();
        held.publish_addr("0.0.0.0:8123".parse().unwrap())
            .await
            .unwrap();
        let err = open(&dir, 4).await.err().unwrap();
        let OpenError::Locked { holder, .. } = &err else {
            panic!("got: {err}");
        };
        assert_eq!(*holder, Some("127.0.0.1:8123".parse().unwrap()));
        assert!(
            err.to_string().contains("http://127.0.0.1:8123"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn a_new_holder_never_reports_its_predecessors_address() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("vectors");
        let (first, _) = open(&dir, 4).await.unwrap();
        first
            .publish_addr("127.0.0.1:8123".parse().unwrap())
            .await
            .unwrap();
        first.close().await.unwrap();

        let (_second, _) = open(&dir, 4).await.unwrap();
        let err = open(&dir, 4).await.err().unwrap();
        assert!(
            matches!(err, OpenError::Locked { holder: None, .. }),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn close_releases_the_lock_even_while_clones_exist() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("vectors");
        let (store, _) = open(&dir, 4).await.unwrap();
        let clone = store.clone();
        store.close().await.unwrap();

        let (_reopened, _) = open(&dir, 4).await.unwrap();
        let err = clone
            .search(&[1.0, 0.0, 0.0, 0.0], 1, SeqFilter::All, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("closed"), "got: {err}");
        clone.close().await.unwrap();
    }

    #[tokio::test]
    async fn recreate_wipes_points_and_reports_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("vectors");
        let (store, _) = open(&dir, 4).await.unwrap();
        store
            .upsert(vec![point("p1", 0, None, vec![1.0, 0.0, 0.0, 0.0])])
            .await
            .unwrap();
        store.close().await.unwrap();

        let (store, opened) = VectorStore::open(&dir, 8, OpenMode::Recreate)
            .await
            .unwrap();
        assert_eq!(opened, Opened::Empty);
        let hits = store
            .search(&[1.0; 8], 10, SeqFilter::All, None)
            .await
            .unwrap();
        assert!(hits.is_empty(), "got: {:?}", hit_ids(&hits));
    }

    #[tokio::test]
    async fn recreate_refuses_a_held_directory_and_leaves_its_data_intact() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("vectors");
        let (held, _) = open(&dir, 4).await.unwrap();
        held.upsert(vec![point("p1", 0, None, vec![1.0, 0.0, 0.0, 0.0])])
            .await
            .unwrap();

        let err = VectorStore::open(&dir, 4, OpenMode::Recreate)
            .await
            .err()
            .unwrap();
        assert!(matches!(err, OpenError::Locked { .. }), "got: {err}");
        let hits = held
            .search(&[1.0, 0.0, 0.0, 0.0], 10, SeqFilter::All, None)
            .await
            .unwrap();
        assert_eq!(hit_ids(&hits), [("p1".to_string(), 0)]);
    }

    #[tokio::test]
    async fn a_corrupt_store_is_unusable_until_recreated() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("vectors");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(SHARD_CONFIG_FILE), "not json").unwrap();

        let err = open(&dir, 4).await.err().unwrap();
        assert!(matches!(err, OpenError::Unusable { .. }), "got: {err}");
        let (_store, opened) = VectorStore::open(&dir, 4, OpenMode::Recreate)
            .await
            .unwrap();
        assert_eq!(opened, Opened::Empty);
    }

    #[tokio::test]
    async fn search_ranks_by_cosine_and_returns_the_page() {
        let (_dir, store) = temp_vectors(4).await;
        store
            .upsert(vec![
                point("far", 1, Some(9), vec![0.0, 1.0, 0.0, 0.0]),
                point("near", 3, Some(7), vec![0.9, 0.1, 0.0, 0.0]),
            ])
            .await
            .unwrap();
        let hits = store
            .search(&[1.0, 0.0, 0.0, 0.0], 10, SeqFilter::All, None)
            .await
            .unwrap();
        assert_eq!(
            hit_ids(&hits),
            [("near".to_string(), 3), ("far".to_string(), 1)]
        );
        assert_eq!(hits[0].page, Some(7));
    }

    #[tokio::test]
    async fn seq_filters_select_summary_or_body_chunks() {
        let (_dir, store) = temp_vectors(4).await;
        store
            .upsert(vec![
                point("p1", 0, None, vec![1.0, 0.0, 0.0, 0.0]),
                point("p1", 1, Some(1), vec![0.9, 0.1, 0.0, 0.0]),
                point("p1", 2, Some(2), vec![0.8, 0.2, 0.0, 0.0]),
            ])
            .await
            .unwrap();
        let q = [1.0, 0.0, 0.0, 0.0];

        let summary = store
            .search(&q, 10, SeqFilter::OnlySummary, None)
            .await
            .unwrap();
        assert_eq!(hit_ids(&summary), [("p1".to_string(), 0)]);
        assert_eq!(summary[0].page, None);

        let body = store
            .search(&q, 10, SeqFilter::OnlyBody, None)
            .await
            .unwrap();
        assert_eq!(
            hit_ids(&body),
            [("p1".to_string(), 1), ("p1".to_string(), 2)]
        );
    }

    #[tokio::test]
    async fn scope_filter_applies_before_truncation() {
        let (_dir, store) = temp_vectors(4).await;
        store
            .upsert(vec![
                point("best", 1, None, vec![1.0, 0.0, 0.0, 0.0]),
                point("inside", 1, None, vec![0.7, 0.3, 0.0, 0.0]),
                point("other", 1, None, vec![0.0, 1.0, 0.0, 0.0]),
            ])
            .await
            .unwrap();
        let scope = ["inside".to_string(), "other".to_string()];
        let hits = store
            .search(&[1.0, 0.0, 0.0, 0.0], 1, SeqFilter::OnlyBody, Some(&scope))
            .await
            .unwrap();
        assert_eq!(hit_ids(&hits), [("inside".to_string(), 1)]);
    }

    #[tokio::test]
    async fn reupserting_the_same_paper_and_seq_overwrites() {
        let (_dir, store) = temp_vectors(4).await;
        store
            .upsert(vec![point("p1", 1, Some(1), vec![0.0, 1.0, 0.0, 0.0])])
            .await
            .unwrap();
        store
            .upsert(vec![point("p1", 1, Some(4), vec![1.0, 0.0, 0.0, 0.0])])
            .await
            .unwrap();
        let hits = store
            .search(&[1.0, 0.0, 0.0, 0.0], 10, SeqFilter::All, None)
            .await
            .unwrap();
        assert_eq!(hit_ids(&hits), [("p1".to_string(), 1)]);
        assert_eq!(hits[0].page, Some(4));
    }

    #[tokio::test]
    async fn upsert_beyond_one_batch_stores_every_point() {
        let (_dir, store) = temp_vectors(4).await;
        let n = UPSERT_BATCH * 2 + 1;
        let points = (1..=n as i64)
            .map(|seq| point("p1", seq, Some(seq), vec![1.0, seq as f32, 0.0, 0.0]))
            .collect();
        store.upsert(points).await.unwrap();
        let hits = store
            .search(&[1.0, 0.0, 0.0, 0.0], n + 10, SeqFilter::All, None)
            .await
            .unwrap();
        assert_eq!(hits.len(), n);
    }

    #[tokio::test]
    async fn delete_paper_removes_only_that_papers_points() {
        let (_dir, store) = temp_vectors(4).await;
        store
            .upsert(vec![
                point("p1", 0, None, vec![1.0, 0.0, 0.0, 0.0]),
                point("p1", 1, Some(1), vec![1.0, 0.1, 0.0, 0.0]),
                point("p2", 0, None, vec![0.0, 1.0, 0.0, 0.0]),
            ])
            .await
            .unwrap();
        store.delete_paper("p1").await.unwrap();
        let hits = store
            .search(&[1.0, 0.0, 0.0, 0.0], 10, SeqFilter::All, None)
            .await
            .unwrap();
        assert_eq!(hit_ids(&hits), [("p2".to_string(), 0)]);
    }

    #[tokio::test]
    async fn replace_paper_leaves_only_the_new_points() {
        let (_dir, store) = temp_vectors(4).await;
        let v = vec![1.0, 0.0, 0.0, 0.0];
        store
            .upsert(vec![
                point("p1", 0, None, v.clone()),
                point("p1", 1, Some(1), v.clone()),
                point("p1", 2, Some(2), v.clone()),
                point("p2", 1, Some(1), v.clone()),
            ])
            .await
            .unwrap();
        let all = async || {
            let mut ids = hit_ids(&store.search(&v, 10, SeqFilter::All, None).await.unwrap());
            ids.sort();
            ids
        };

        store
            .replace_paper("p1", vec![point("p1", 1, Some(1), v.clone())])
            .await
            .unwrap();
        assert_eq!(all().await, [("p1".to_string(), 1), ("p2".to_string(), 1)]);
        store.replace_paper("p1", Vec::new()).await.unwrap();
        assert_eq!(all().await, [("p2".to_string(), 1)]);
    }

    #[tokio::test]
    async fn replace_summary_rewrites_or_removes_only_seq_zero() {
        let (_dir, store) = temp_vectors(4).await;
        store
            .upsert(vec![
                point("p1", 0, None, vec![1.0, 0.0, 0.0, 0.0]),
                point("p1", 1, Some(1), vec![0.0, 1.0, 0.0, 0.0]),
            ])
            .await
            .unwrap();
        store
            .replace_summary("p1", Some(point("p1", 0, None, vec![0.0, 0.0, 1.0, 0.0])))
            .await
            .unwrap();
        let top = store
            .search(&[0.0, 0.0, 1.0, 0.0], 1, SeqFilter::OnlySummary, None)
            .await
            .unwrap();
        assert_eq!(hit_ids(&top), [("p1".to_string(), 0)]);
        assert_eq!(store.scroll_summaries().await.unwrap()[0].1[2], 1.0);

        store.replace_summary("p1", None).await.unwrap();
        assert!(store.scroll_summaries().await.unwrap().is_empty());
        let body = store
            .search(&[0.0, 1.0, 0.0, 0.0], 10, SeqFilter::All, None)
            .await
            .unwrap();
        assert_eq!(hit_ids(&body), [("p1".to_string(), 1)]);
    }

    #[tokio::test]
    async fn a_waiting_store_takes_over_once_the_holder_lets_go() {
        let dir = tempfile::tempdir().unwrap();
        let (holder, _) = open(dir.path(), 4).await.unwrap();
        let waiting = VectorStore::waiting(dir.path(), 4);
        assert!(waiting.is_waiting());
        let reason = waiting.unavailable().unwrap();
        assert!(reason.contains("in use"), "got: {reason}");
        let err = waiting
            .search(&[1.0, 0.0, 0.0, 0.0], 1, SeqFilter::All, None)
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), reason);
        let addr: SocketAddr = "127.0.0.1:8123".parse().unwrap();
        waiting.publish_addr(addr).await.unwrap();
        assert_eq!(waiting.acquire().await.unwrap(), None, "still held");

        holder.close().await.unwrap();
        assert_eq!(waiting.acquire().await.unwrap(), Some(Opened::Empty));
        assert!(!waiting.is_waiting() && waiting.unavailable().is_none());
        match open(dir.path(), 4).await {
            Err(OpenError::Locked { holder, .. }) => {
                assert_eq!(holder, Some(addr), "the deferred address is published")
            }
            other => panic!("expected Locked, got {:?}", other.map(|(_, o)| o)),
        }
        assert_eq!(
            waiting.acquire().await.unwrap(),
            None,
            "nothing to take over"
        );
    }

    #[tokio::test]
    async fn scroll_summaries_pages_past_one_page_and_skips_body_chunks() {
        let (_dir, store) = temp_vectors(4).await;
        let n = SCROLL_PAGE + 44;
        let mut points: Vec<ChunkPoint> = (0..n)
            .map(|i| point(&format!("p{i}"), 0, None, vec![0.0, 0.0, 2.0, 0.0]))
            .collect();
        points.push(point("p0", 1, Some(1), vec![1.0, 0.0, 0.0, 0.0]));
        store.upsert(points).await.unwrap();

        let out = store.scroll_summaries().await.unwrap();
        assert_eq!(out.len(), n);
        let ids: std::collections::HashSet<&str> = out.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids.len(), n);
        assert!(out.iter().all(|(_, v)| v == &[0.0, 0.0, 1.0, 0.0]));
    }

    #[tokio::test]
    async fn optimize_keeps_a_populated_store_searchable() {
        let (_dir, store) = temp_vectors(4).await;
        store
            .upsert(vec![
                point("p1", 0, None, vec![1.0, 0.0, 0.0, 0.0]),
                point("p2", 0, None, vec![0.0, 1.0, 0.0, 0.0]),
            ])
            .await
            .unwrap();
        store.delete_paper("p2").await.unwrap();
        store.optimize().await.unwrap();
        let hits = store
            .search(&[1.0, 0.0, 0.0, 0.0], 10, SeqFilter::All, None)
            .await
            .unwrap();
        assert_eq!(hit_ids(&hits), [("p1".to_string(), 0)]);
    }
}
