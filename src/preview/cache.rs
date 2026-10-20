//! Where rendered pages live between requests.
//!
//! Two caches with different lifetimes: PNGs on disk, keyed by the PDF's
//! `content_hash`, and page geometry in process. The disk key is what makes
//! invalidation a non-problem — different bytes hash differently, so a stale
//! entry is unreachable rather than wrong, and the whole directory is safe to
//! delete at any time (it re-renders on demand), exactly like the Tantivy
//! index.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use super::render::PageMeta;

/// Every rendered page of one document. Sharded by hash rather than by paper
/// id so two ingests of the same bytes share one render — the same identity
/// `papers::find_by_hash` dedupes on. `papers.content_hash` is UNIQUE, so
/// purging a paper can remove this whole directory without stranding another
/// row.
pub fn paper_dir(cache_dir: &Path, content_hash: &str) -> PathBuf {
    cache_dir.join(content_hash)
}

/// `<cache_dir>/<content_hash>/<page>.png`.
fn page_path(cache_dir: &Path, content_hash: &str, page: u32) -> PathBuf {
    paper_dir(cache_dir, content_hash).join(format!("{page}.png"))
}

pub(crate) async fn read(cache_dir: &Path, content_hash: &str, page: u32) -> Option<Vec<u8>> {
    tokio::fs::read(page_path(cache_dir, content_hash, page))
        .await
        .ok()
}

/// Publish a rendered page. Written to a sibling temp file and renamed, so a
/// concurrent reader never sees a half-written PNG and two renders of the
/// same page can't interleave into one corrupt file.
pub(crate) async fn write(
    cache_dir: &Path,
    content_hash: &str,
    page: u32,
    bytes: &[u8],
) -> std::io::Result<()> {
    let dst = page_path(cache_dir, content_hash, page);
    let dir = dst.parent().expect("page path always has a parent");
    tokio::fs::create_dir_all(dir).await?;
    // The pid keeps two processes sharing a cache dir (a `serve` and a CLI
    // render, say) from racing on one staging name.
    let staging = dir.join(format!(".{page}.{}.png.tmp", std::process::id()));
    tokio::fs::write(&staging, bytes).await?;
    match tokio::fs::rename(&staging, &dst).await {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = tokio::fs::remove_file(&staging).await;
            Err(e)
        }
    }
}

/// Page counts and geometry, remembered for the service's lifetime. Bounded
/// by the number of distinct PDFs ever previewed and holding three numbers
/// each, so it is never evicted — the same reasoning as `web::assets`'s
/// compressed-body map, which this mirrors. Owned by `PreviewService` rather
/// than process-wide: a `static` outlived the service, and the integration
/// suite builds one server per test around the same paper hash, so one
/// server's answer leaked into the next.
#[derive(Default)]
pub(crate) struct MetaCache(RwLock<HashMap<String, PageMeta>>);

impl MetaCache {
    pub(crate) fn get(&self, content_hash: &str) -> Option<PageMeta> {
        self.0.read().ok()?.get(content_hash).copied()
    }

    pub(crate) fn put(&self, content_hash: &str, meta: PageMeta) {
        if let Ok(mut m) = self.0.write() {
            m.insert(content_hash.to_string(), meta);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn write_then_read_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "abc", 2, b"png-bytes").await.unwrap();
        assert_eq!(
            read(dir.path(), "abc", 2).await.as_deref(),
            Some(&b"png-bytes"[..])
        );
    }

    #[tokio::test]
    async fn a_miss_reads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path(), "abc", 0).await.is_none());
    }

    #[tokio::test]
    async fn leaves_no_staging_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "abc", 0, b"x").await.unwrap();
        let entries: Vec<_> = std::fs::read_dir(dir.path().join("abc"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(entries, vec!["0.png".to_string()]);
    }

    #[test]
    fn meta_round_trips_by_hash() {
        let meta = PageMeta {
            pages: 4,
            width: 595.0,
            height: 842.0,
        };
        let cache = MetaCache::default();
        cache.put("hash-for-meta-test", meta);
        assert_eq!(cache.get("hash-for-meta-test"), Some(meta));
        assert_eq!(cache.get("never-stored"), None);
    }
}
