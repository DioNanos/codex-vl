//! Optional dense scorer for hybrid retrieval (feature `embeddings`).
//!
//! When the AI client requests `dense_alpha > 0` on `msa_search` or
//! `msa_search_iterative`, the server fetches up to `top_k * 4` BM25
//! candidates from tantivy, computes the query embedding via the
//! configured [`EmbeddingScorer`], reads each candidate's cached chunk
//! embedding, and rescores:
//!
//! ```text
//! score = α · bm25_normalized + (1 - α) · ((cosine + 1) / 2)
//! ```
//!
//! Cosine similarity is shifted to `[0, 1]` so it composes linearly with
//! the already-normalized BM25 score. Chunks without a cached embedding
//! fall back to BM25 only (`cosine = 0` shifted = `0.5`); the alpha can be
//! pushed to `1.0` to keep BM25 behavior unchanged.
//!
//! Embeddings are computed at *index time* and stored in tantivy as a
//! little-endian f32 byte vector. The scorer is bound at server startup;
//! a mismatched embedding dimension on a re-opened index returns an error
//! at search time rather than crashing.

use std::sync::Arc;

#[allow(unused_imports)]
use crate::error::{MsaError, Result};

/// Sync embedding scorer abstraction. Sync because index_document and the
/// search hot path are not async and we do not want to drag tokio into
/// every call site. The Ollama implementation uses `reqwest::blocking`
/// which spins up its own internal runtime; this is fine for an MCP stdio
/// server where requests are sequential.
pub trait EmbeddingScorer: Send + Sync {
    /// Compute an embedding for `text`. Returns `MsaError::Config` if the
    /// service is misconfigured / down — callers should be ready to fall
    /// back to pure BM25.
    fn embed(&self, text: &str) -> Result<Vec<f32>>;

    /// Reported embedding dimension. Used to validate cached embeddings
    /// at search time.
    fn dim(&self) -> usize;

    /// Human-readable identifier for diagnostics.
    fn name(&self) -> &str;
}

/// Cosine similarity shifted to `[0.0, 1.0]`. Vectors must have equal
/// length and non-zero norm; otherwise returns `0.5` (the neutral value).
pub fn cosine_unit(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.5;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.5;
    }
    let cos = dot / (na.sqrt() * nb.sqrt());
    ((cos + 1.0) / 2.0).clamp(0.0, 1.0)
}

/// Pack an `&[f32]` as little-endian bytes for tantivy storage.
pub fn pack_f32(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for &x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// Unpack `&[u8]` to `Vec<f32>`. Returns empty on length mismatch.
pub fn unpack_f32(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[cfg(feature = "embeddings")]
mod ollama {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    /// Embedding client for Ollama (`POST /api/embeddings`). Other backends
    /// that expose a compatible JSON shape (`{model, prompt}` → `{embedding}`)
    /// will work as well.
    pub struct OllamaClient {
        base_url: String,
        model: String,
        dim: usize,
        client: reqwest::blocking::Client,
    }

    #[derive(Serialize)]
    struct EmbedReq<'a> {
        model: &'a str,
        prompt: &'a str,
    }

    #[derive(Deserialize)]
    struct EmbedResp {
        embedding: Vec<f32>,
    }

    impl OllamaClient {
        pub fn new(
            base_url: impl Into<String>,
            model: impl Into<String>,
            dim: usize,
        ) -> Result<Self> {
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .map_err(|e| MsaError::Config(format!("reqwest build: {e}")))?;
            Ok(Self {
                base_url: base_url.into(),
                model: model.into(),
                dim,
                client,
            })
        }
    }

    impl EmbeddingScorer for OllamaClient {
        fn embed(&self, text: &str) -> Result<Vec<f32>> {
            let url = format!("{}/api/embeddings", self.base_url.trim_end_matches('/'));
            let resp: EmbedResp = self
                .client
                .post(&url)
                .json(&EmbedReq {
                    model: &self.model,
                    prompt: text,
                })
                .send()
                .and_then(|r| r.error_for_status())
                .and_then(|r| r.json::<EmbedResp>())
                .map_err(|e| MsaError::Config(format!("ollama embed: {e}")))?;
            if resp.embedding.len() != self.dim {
                return Err(MsaError::Config(format!(
                    "ollama returned dim {} but configured dim is {}",
                    resp.embedding.len(),
                    self.dim
                )));
            }
            Ok(resp.embedding)
        }

        fn dim(&self) -> usize {
            self.dim
        }

        fn name(&self) -> &str {
            "ollama"
        }
    }
}

#[cfg(feature = "embeddings")]
pub use ollama::OllamaClient;

/// Convenience alias used across the crate.
pub type SharedScorer = Arc<dyn EmbeddingScorer>;

/// Deterministic in-process scorer used by unit tests of other modules.
/// Public-in-crate so `mod tests` in `index.rs` can reach it without a
/// duplicate definition. Gated to test builds.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// One-hot vector keyed on the hash of the first whitespace word.
    /// Two inputs that share their first word produce identical embeddings,
    /// which lets tests construct deterministic dense-rerank scenarios.
    pub struct MockScorer {
        dim: usize,
    }

    impl MockScorer {
        pub fn new(dim: usize) -> Self {
            Self { dim }
        }
    }

    impl EmbeddingScorer for MockScorer {
        fn embed(&self, text: &str) -> Result<Vec<f32>> {
            let mut v = vec![0.0; self.dim];
            if let Some(w) = text.split_whitespace().next() {
                let h = w
                    .bytes()
                    .fold(0u64, |a, b| a.wrapping_mul(33).wrapping_add(b as u64));
                v[(h as usize) % self.dim] = 1.0;
            }
            Ok(v)
        }

        fn dim(&self) -> usize {
            self.dim
        }

        fn name(&self) -> &str {
            "mock"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::MockScorer;
    use super::*;

    #[test]
    fn cosine_identity_is_one() {
        let v = vec![0.1, 0.2, 0.3, 0.4];
        assert!((cosine_unit(&v, &v) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn cosine_orthogonal_is_half() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        assert!((cosine_unit(&a, &b) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn cosine_opposite_is_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        assert!(cosine_unit(&a, &b) < 1e-5);
    }

    #[test]
    fn pack_unpack_roundtrip() {
        let v = vec![0.1, -0.2, 1.5, -42.7, 0.0];
        assert_eq!(unpack_f32(&pack_f32(&v)), v);
    }

    #[test]
    fn mock_scorer_deterministic() {
        let s = MockScorer::new(8);
        let a = s.embed("hello world").unwrap();
        let b = s.embed("hello there").unwrap();
        assert_eq!(a, b, "first word identical → same embedding");
        let c = s.embed("alpha bravo").unwrap();
        assert_ne!(a, c, "different first word → different embedding");
    }
}
