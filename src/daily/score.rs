use anyhow::Result;
use sqlx::SqlitePool;
use std::collections::HashMap;

use crate::search::vector::QdrantStore;

/// zotero-arxiv-daily's recency weights for `n` corpus papers ranked
/// newest-first: w_i = 1/(1+log10(i+1)), normalized to sum 1.
fn recency_weights(n: usize) -> Vec<f32> {
    let raw: Vec<f32> = (0..n)
        .map(|i| 1.0 / (1.0 + ((i + 1) as f32).log10()))
        .collect();
    let sum: f32 = raw.iter().sum();
    raw.into_iter().map(|w| w / sum).collect()
}

pub(crate) fn l2_normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

pub(crate) fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Interest-profile vector: the recency-weighted sum of the library's
/// normalized seq-0 vectors. Scoring a candidate against it with `dot`
/// equals the weighted mean cosine similarity over the whole library.
/// `None` when no live paper has an indexed summary vector.
pub(crate) async fn build_profile(
    pool: &SqlitePool,
    vectors: &QdrantStore,
) -> Result<Option<Vec<f32>>> {
    let points = vectors.scroll_summaries().await?;
    let mut by_id: HashMap<String, Vec<f32>> = points.into_iter().collect();

    let ids: Vec<(String,)> =
        sqlx::query_as("SELECT id FROM papers WHERE deleted_at IS NULL ORDER BY added_at DESC, id")
            .fetch_all(pool)
            .await?;

    // Newest-first vectors for live papers that are actually indexed.
    let mut ranked: Vec<Vec<f32>> = Vec::new();
    for (id,) in ids {
        if let Some(mut v) = by_id.remove(&id) {
            l2_normalize(&mut v);
            ranked.push(v);
        }
    }
    if ranked.is_empty() {
        return Ok(None);
    }

    let weights = recency_weights(ranked.len());
    let mut profile = vec![0.0f32; ranked[0].len()];
    for (v, w) in ranked.iter().zip(&weights) {
        for (p, x) in profile.iter_mut().zip(v) {
            *p += w * x;
        }
    }
    Ok(Some(profile))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn recency_weights_normalized_and_decreasing() {
        let w = recency_weights(3);
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert!(w[0] > w[1] && w[1] > w[2]);
    }

    #[test]
    fn profile_score_equals_weighted_mean_of_cosines() {
        // Unit corpus vectors, newest first.
        let v1 = vec![1.0f32, 0.0];
        let v2 = vec![0.6f32, 0.8];
        let mut cand = vec![0.8f32, 0.6];
        l2_normalize(&mut cand);
        let w = recency_weights(2);
        let explicit = w[0] * dot(&cand, &v1) + w[1] * dot(&cand, &v2);

        let mut profile = vec![0.0f32; 2];
        for (v, wi) in [v1, v2].iter().zip(&w) {
            for (p, x) in profile.iter_mut().zip(v) {
                *p += wi * x;
            }
        }
        assert!((dot(&cand, &profile) - explicit).abs() < 1e-5);
    }

    async fn pool() -> sqlx::SqlitePool {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let p = crate::db::connect(&url).await.unwrap();
        std::mem::forget(dir);
        p
    }

    fn paper(id: &str, added_at: &str) -> crate::library::model::Paper {
        crate::library::model::Paper {
            id: id.into(),
            content_hash: format!("h-{id}"),
            rel_path: format!("{id}.pdf"),
            cite_key: None,
            added_at: added_at.into(),
            deleted_at: None,
            starred: false,
            name: None,
            meta: crate::library::model::PaperMeta {
                title: Some("T".into()),
                abstract_text: None,
                authors: crate::library::model::Authors(vec![]),
                venue: None,
                year: None,
                doi: None,
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: crate::library::model::PaperStatus::Resolved,
            },
        }
    }

    fn scroll_mock(points: serde_json::Value) -> Mock {
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/scroll"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {"points": points, "next_page_offset": null}
            })))
    }

    #[tokio::test]
    async fn newer_library_paper_dominates_profile() {
        let pool = pool().await;
        crate::library::papers::insert(&pool, &paper("new1", "2026-07-09T00:00:00Z"))
            .await
            .unwrap();
        crate::library::papers::insert(&pool, &paper("old1", "2026-01-01T00:00:00Z"))
            .await
            .unwrap();

        let server = MockServer::start().await;
        scroll_mock(json!([
            {"id": "a", "payload": {"paper_id": "new1", "seq": 0}, "vector": [1.0, 0.0, 0.0, 0.0]},
            {"id": "b", "payload": {"paper_id": "old1", "seq": 0}, "vector": [0.0, 1.0, 0.0, 0.0]}
        ]))
        .mount(&server)
        .await;
        let vectors = crate::search::vector::QdrantStore::new(&server.uri(), "xuewen", 4).unwrap();

        let profile = build_profile(&pool, &vectors).await.unwrap().unwrap();
        // Candidate matching the NEW paper must outrank one matching the OLD.
        let like_new = dot(&[1.0, 0.0, 0.0, 0.0], &profile);
        let like_old = dot(&[0.0, 1.0, 0.0, 0.0], &profile);
        assert!(like_new > like_old, "{like_new} vs {like_old}");
    }

    #[tokio::test]
    async fn empty_library_gives_no_profile() {
        let pool = pool().await;
        let server = MockServer::start().await;
        scroll_mock(json!([])).mount(&server).await;
        let vectors = crate::search::vector::QdrantStore::new(&server.uri(), "xuewen", 4).unwrap();
        assert!(build_profile(&pool, &vectors).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn trashed_papers_are_excluded_from_profile() {
        let pool = pool().await;
        crate::library::papers::insert(&pool, &paper("p1", "2026-07-09T00:00:00Z"))
            .await
            .unwrap();
        crate::library::trash::soft_delete(&pool, "p1")
            .await
            .unwrap();
        let server = MockServer::start().await;
        scroll_mock(json!([
            {"id": "a", "payload": {"paper_id": "p1", "seq": 0}, "vector": [1.0, 0.0, 0.0, 0.0]}
        ]))
        .mount(&server)
        .await;
        let vectors = crate::search::vector::QdrantStore::new(&server.uri(), "xuewen", 4).unwrap();
        assert!(build_profile(&pool, &vectors).await.unwrap().is_none());
    }
}
