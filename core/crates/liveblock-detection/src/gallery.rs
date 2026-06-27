//! Team-protect Stage-2a gallery matching.
//!
//! Pure nearest-neighbor math: a [`GalleryMatcher`] holds a set of reference
//! embeddings (one per protected team emblem / league mark) produced by an
//! embedding model elsewhere. At runtime we embed a candidate region and ask for
//! its cosine similarity to the closest gallery entry; the policy layer
//! (`liveblock_core::decide_verdict`) turns that scalar into a Keep/Remove/Unsure
//! verdict via `ClassifySignals::team_gallery_sim`.
//!
//! This module contains NO model code — just the vector algebra — so it stays
//! pure-Rust and unit-testable without ort/GPU.

/// Cosine similarity of two equal-length vectors, in `[-1, 1]`.
///
/// Returns `0.0` (orthogonal / "no signal") when the lengths differ or either
/// vector has zero magnitude — a safe neutral value that will not, on its own,
/// push the policy toward a removal.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for (&x, &y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom <= 0.0 || !denom.is_finite() {
        return 0.0;
    }
    let sim = dot / denom;
    // Guard against tiny floating-point overshoot beyond [-1, 1].
    sim.clamp(-1.0, 1.0)
}

/// A bank of reference embeddings for the protected team / league gallery.
///
/// All embeddings are expected to share a dimensionality; entries whose length
/// differs from a query are skipped (they contribute similarity `0.0`).
#[derive(Debug, Clone, Default)]
pub struct GalleryMatcher {
    /// One row per reference image embedding.
    pub embeddings: Vec<Vec<f32>>,
}

impl GalleryMatcher {
    /// Empty gallery — every query scores `0.0`.
    pub fn new() -> Self {
        Self {
            embeddings: Vec::new(),
        }
    }

    /// Build a matcher from a set of reference embeddings.
    pub fn with_embeddings(embeddings: Vec<Vec<f32>>) -> Self {
        Self { embeddings }
    }

    /// Add one reference embedding.
    pub fn push(&mut self, embedding: Vec<f32>) {
        self.embeddings.push(embedding);
    }

    /// Number of reference embeddings.
    pub fn len(&self) -> usize {
        self.embeddings.len()
    }

    /// True when the gallery has no references (every query scores `0.0`).
    pub fn is_empty(&self) -> bool {
        self.embeddings.is_empty()
    }

    /// Maximum cosine similarity of `query` to any gallery embedding.
    ///
    /// Returns `0.0` for an empty gallery (the neutral "no protected match"
    /// value). This is exactly the scalar fed to
    /// `ClassifySignals::team_gallery_sim`.
    pub fn max_similarity(&self, query: &[f32]) -> f32 {
        let mut best = 0.0f32;
        for emb in &self.embeddings {
            let sim = cosine_similarity(query, emb);
            if sim > best {
                best = sim;
            }
        }
        best
    }

    /// Like [`max_similarity`](Self::max_similarity) but also returns the index
    /// of the best-matching gallery entry, or `None` if the gallery is empty.
    pub fn best_match(&self, query: &[f32]) -> Option<(usize, f32)> {
        let mut best: Option<(usize, f32)> = None;
        for (i, emb) in self.embeddings.iter().enumerate() {
            let sim = cosine_similarity(query, emb);
            match best {
                Some((_, b)) if sim <= b => {}
                _ => best = Some((i, sim)),
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_identical_vectors_is_one() {
        let a = [1.0, 2.0, 3.0];
        assert!((cosine_similarity(&a, &a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_orthogonal_is_zero() {
        let a = [1.0, 0.0];
        let b = [0.0, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn cosine_opposite_is_minus_one() {
        let a = [1.0, 1.0];
        let b = [-1.0, -1.0];
        assert!((cosine_similarity(&a, &b) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_is_scale_invariant() {
        let a = [1.0, 2.0, 3.0];
        let b = [2.0, 4.0, 6.0]; // 2x scale
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_mismatched_len_is_zero() {
        assert_eq!(cosine_similarity(&[1.0, 2.0], &[1.0]), 0.0);
    }

    #[test]
    fn cosine_zero_vector_is_zero() {
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn cosine_clamped_to_unit_range() {
        // Deliberately large values; the result must never exceed 1.0.
        let a = [1e20, 1e20, 1e20];
        let s = cosine_similarity(&a, &a);
        assert!((-1.0..=1.0).contains(&s), "got {s}");
    }

    #[test]
    fn empty_gallery_scores_zero() {
        let g = GalleryMatcher::new();
        assert!(g.is_empty());
        assert_eq!(g.max_similarity(&[1.0, 2.0, 3.0]), 0.0);
        assert!(g.best_match(&[1.0, 2.0, 3.0]).is_none());
    }

    #[test]
    fn max_similarity_picks_closest() {
        // Query points exactly along entry 1's direction, far from the others,
        // so entry 1 is unambiguously the best match (cosine == 1.0).
        let g = GalleryMatcher::with_embeddings(vec![
            vec![1.0, 0.0, 0.0],  // orthogonal-ish to query
            vec![1.0, 2.0, 0.0],  // exact direction of the query (closest)
            vec![0.0, 0.0, 1.0],  // orthogonal
        ]);
        let query = [2.0, 4.0, 0.0]; // = 2 * entry 1
        let best = g.max_similarity(&query);
        assert!((best - 1.0).abs() < 1e-6, "got {best}");
        let (idx, sim) = g.best_match(&query).unwrap();
        assert_eq!(idx, 1);
        assert!((sim - best).abs() < 1e-6);
    }

    #[test]
    fn max_similarity_never_negative_floor() {
        // All gallery entries point opposite the query; max stays at the 0.0
        // neutral floor rather than going negative (so it never *adds* removal
        // pressure for a confidently-not-a-team region).
        let g = GalleryMatcher::with_embeddings(vec![vec![-1.0, -1.0], vec![-2.0, -3.0]]);
        let s = g.max_similarity(&[1.0, 1.0]);
        assert_eq!(s, 0.0);
    }

    #[test]
    fn push_and_len() {
        let mut g = GalleryMatcher::new();
        g.push(vec![1.0, 0.0]);
        g.push(vec![0.0, 1.0]);
        assert_eq!(g.len(), 2);
        assert!(!g.is_empty());
    }
}
