use std::collections::HashMap;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub type DocId = String;
pub type CollectionName = String;

/// A document submitted for indexing. Chunking happens server-side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: DocId,
    pub text: String,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,
}

/// A single chunk hit returned by `search`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkHit {
    pub doc_id: DocId,
    pub chunk_idx: usize,
    /// Normalized 0.0–1.0 score (per-query max-scaled).
    pub score: f32,
    /// Raw BM25 score from tantivy, kept for debugging / re-ranking.
    pub raw_score: f32,
    pub snippet: String,
    /// `(start, end)` in the chunk text, in character offsets. End is exclusive.
    pub char_offset: (usize, usize),
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Pre-retrieval filter on document metadata.
///
/// Semantics: a document passes the filter iff *all* `where_eq` keys match
/// AND *all* `where_in` keys match one of the listed values AND the
/// `created_at` falls within the optional date range.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SearchFilter {
    /// Each entry must match exactly: `metadata[key] == value`.
    #[serde(default)]
    pub where_eq: HashMap<String, serde_json::Value>,
    /// Each entry must match one of the allowed values: `metadata[key] ∈ values`.
    #[serde(default)]
    pub where_in: HashMap<String, Vec<serde_json::Value>>,
    pub created_after: Option<DateTime<Utc>>,
    pub created_before: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsaStats {
    pub collection: CollectionName,
    pub num_documents: u64,
    pub num_chunks: u64,
    pub total_tokens: u64,
    pub disk_bytes: u64,
}

/// Per-collection chunking configuration. Defaults follow MSA paper P=64.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkConfig {
    pub chunk_size: usize,
    pub overlap: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            chunk_size: 64,
            overlap: 0,
        }
    }
}
