use std::collections::HashMap;

/// Reciprocal Rank Fusion: score(id) = Σ over lists 1/(k + rank), rank 1-based.
/// Items appearing in several lists rise; no score normalization needed.
/// Returns ids only — a single non-empty list comes back in its own order
/// (scores are strictly decreasing in rank), so callers need no special case.
pub(crate) fn rrf(lists: &[Vec<String>], k: f32) -> Vec<String> {
    let mut scores: HashMap<String, f32> = HashMap::new();
    for list in lists {
        for (i, id) in list.iter().enumerate() {
            *scores.entry(id.clone()).or_default() += 1.0 / (k + (i as f32) + 1.0);
        }
    }
    let mut out: Vec<(String, f32)> = scores.into_iter().collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then_with(|| a.0.cmp(&b.0)));
    out.into_iter().map(|(id, _)| id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_list_preserves_order() {
        let out = rrf(&[vec!["a".into(), "b".into(), "c".into()]], 60.0);
        assert_eq!(out, vec!["a", "b", "c"]);
    }

    #[test]
    fn paper_in_both_lists_outranks_single_list_leaders() {
        // "x" is rank 2 in both lists; "a" and "b" lead one list each.
        let out = rrf(
            &[
                vec!["a".into(), "x".into(), "c".into()],
                vec!["b".into(), "x".into(), "d".into()],
            ],
            60.0,
        );
        assert_eq!(out[0], "x"); // 2/(60+2) beats 1/(60+1)
    }

    #[test]
    fn empty_input_is_empty() {
        assert!(rrf(&[], 60.0).is_empty());
        assert!(rrf(&[vec![], vec![]], 60.0).is_empty());
    }

    #[test]
    fn ties_break_by_id_for_determinism() {
        let out = rrf(&[vec!["b".into()], vec!["a".into()]], 60.0);
        assert_eq!(out, vec!["a", "b"]); // equal scores → lexicographic
    }
}
