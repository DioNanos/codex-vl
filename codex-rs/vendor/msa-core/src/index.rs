//! Per-collection tantivy index. One on-disk index per collection.
//!
//! Schema (tantivy fields):
//! - `doc_id`     STRING | STORED  — sanitized document id
//! - `chunk_idx`  FAST   | STORED  — `u64`. Chunks use 0..N. The full-doc
//!   sentinel uses `u64::MAX`.
//! - `is_sentinel` FAST            — 0 for chunk records, 1 for the full-doc
//!   sentinel. Search excludes sentinels.
//! - `chunk_text` TEXT   | STORED  — chunk content, BM25-searchable
//! - `full_text`  STORED           — only set on the sentinel
//! - `char_start`, `char_end` FAST | STORED — `u64`
//! - `created_at` FAST   | STORED  — unix seconds
//! - `metadata`   STORED           — serialized JSON object
//!
//! Persistence is handled by tantivy itself (mmap on disk). No WAL needed
//! for v0.2; tantivy commits are atomic.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, OwnedValue, STORED, STRING, Schema, TEXT};
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term, doc};

use crate::embeddings::{EmbeddingScorer, cosine_unit, pack_f32, unpack_f32};

use crate::chunker::chunk_text;
use crate::error::{MsaError, Result};
use crate::schema::{ChunkConfig, ChunkHit, Document, MsaStats, SearchFilter};

/// Sentinel value in `chunk_idx` that marks the full-document record.
const SENTINEL_CHUNK_IDX: u64 = u64::MAX;

#[derive(Clone)]
pub struct MsaIndex {
    name: String,
    path: PathBuf,
    chunking: ChunkConfig,
    fields: Fields,
    index: Index,
    reader: IndexReader,
}

#[derive(Clone)]
struct Fields {
    doc_id: Field,
    chunk_idx: Field,
    is_sentinel: Field,
    chunk_text: Field,
    full_text: Field,
    char_start: Field,
    char_end: Field,
    created_at: Field,
    metadata: Field,
    /// Always present in the schema (forward-compat). Holds packed f32
    /// little-endian bytes when the dense scorer was active at index time;
    /// empty otherwise. Search-time hybrid scoring tolerates missing
    /// embeddings by falling back to BM25.
    embedding: Field,
}

impl MsaIndex {
    /// Open or create the on-disk tantivy index for `name` rooted at `path`.
    pub fn open(name: String, path: PathBuf, chunking: ChunkConfig) -> Result<Self> {
        std::fs::create_dir_all(&path)?;

        let mut sb = Schema::builder();
        let doc_id = sb.add_text_field("doc_id", STRING | STORED);
        let chunk_idx = sb.add_u64_field("chunk_idx", tantivy::schema::FAST | STORED);
        let is_sentinel = sb.add_u64_field("is_sentinel", tantivy::schema::FAST | STORED);
        let chunk_text = sb.add_text_field("chunk_text", TEXT | STORED);
        let full_text = sb.add_text_field("full_text", STORED);
        let char_start = sb.add_u64_field("char_start", tantivy::schema::FAST | STORED);
        let char_end = sb.add_u64_field("char_end", tantivy::schema::FAST | STORED);
        let created_at = sb.add_i64_field("created_at", tantivy::schema::FAST | STORED);
        let metadata = sb.add_text_field("metadata", STORED);
        let embedding = sb.add_bytes_field("embedding", STORED);
        let schema = sb.build();

        let index = match Index::open_in_dir(&path) {
            Ok(idx) => idx,
            Err(_) => Index::create_in_dir(&path, schema.clone())?,
        };

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        Ok(Self {
            name,
            path,
            chunking,
            fields: Fields {
                doc_id,
                chunk_idx,
                is_sentinel,
                chunk_text,
                full_text,
                char_start,
                char_end,
                created_at,
                metadata,
                embedding,
            },
            index,
            reader,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Index (or reindex) a document. Existing chunks for this `doc_id` are
    /// removed first to keep the index consistent.
    ///
    /// When `embedder` is `Some`, each chunk's dense embedding is computed
    /// and persisted alongside the BM25 token stream so hybrid scoring at
    /// search time can rerank without re-querying the embedding service.
    /// If the embedder fails on a chunk, the call returns an error and the
    /// in-progress writer is rolled back (no partial commit).
    pub fn index_document(
        &self,
        doc: &Document,
        embedder: Option<&dyn EmbeddingScorer>,
    ) -> Result<usize> {
        let mut writer: IndexWriter = self.index.writer(50_000_000)?;
        let term = Term::from_field_text(self.fields.doc_id, &doc.id);
        writer.delete_term(term);

        let chunks = chunk_text(&doc.text, &self.chunking);
        let metadata_json = serde_json::to_string(&doc.metadata)?;
        let created_unix = doc.created_at.timestamp();

        for (idx, c) in chunks.iter().enumerate() {
            let embedding_bytes: Vec<u8> = match embedder {
                Some(e) => pack_f32(&e.embed(&c.text)?),
                None => Vec::new(),
            };
            writer.add_document(doc!(
                self.fields.doc_id => doc.id.clone(),
                self.fields.chunk_idx => idx as u64,
                self.fields.is_sentinel => 0u64,
                self.fields.chunk_text => c.text.clone(),
                self.fields.char_start => c.char_offset.0 as u64,
                self.fields.char_end => c.char_offset.1 as u64,
                self.fields.created_at => created_unix,
                self.fields.metadata => metadata_json.clone(),
                self.fields.embedding => embedding_bytes,
            ))?;
        }

        // Sentinel record holding the full text for `fetch_doc`. No embedding.
        writer.add_document(doc!(
            self.fields.doc_id => doc.id.clone(),
            self.fields.chunk_idx => SENTINEL_CHUNK_IDX,
            self.fields.is_sentinel => 1u64,
            self.fields.full_text => doc.text.clone(),
            self.fields.created_at => created_unix,
            self.fields.metadata => metadata_json,
            self.fields.embedding => Vec::<u8>::new(),
        ))?;

        writer.commit()?;
        self.maybe_merge_segments(&mut writer);
        Ok(chunks.len())
    }

    /// Past this many searchable segments a successful write triggers an
    /// explicit, synchronous merge (F2, vivling live audit 2026-06-07: a
    /// commit-per-capsule workload accumulated 4027 segments for 4233 docs,
    /// opening ~4k segment readers per recall). Explicit by design: the
    /// native LogMergePolicy merges asynchronously at uncontrolled moments;
    /// here the merge runs only inside our own write path. Best-effort — a
    /// failure leaves the index correct, just fragmented. Mirrors the
    /// canonical implementation in the standalone msa-core (022f0bc).
    const MAX_SEGMENTS_BEFORE_MERGE: usize = 24;

    fn maybe_merge_segments(&self, writer: &mut IndexWriter) {
        let ids = match self.index.searchable_segment_ids() {
            Ok(ids) => ids,
            Err(e) => {
                tracing::warn!("segment id listing failed, skipping merge: {e}");
                return;
            }
        };
        if ids.len() <= Self::MAX_SEGMENTS_BEFORE_MERGE {
            return;
        }
        tracing::info!(
            "merging {} segments (threshold {})",
            ids.len(),
            Self::MAX_SEGMENTS_BEFORE_MERGE
        );
        match writer.merge(&ids).wait() {
            Ok(_) => {
                if let Err(e) = self.reader.reload() {
                    tracing::warn!("reader reload after merge failed: {e}");
                }
            }
            Err(e) => tracing::warn!("segment merge failed (index stays fragmented): {e}"),
        }
    }

    pub fn delete_document(&self, doc_id: &str) -> Result<()> {
        let mut writer: IndexWriter = self.index.writer(50_000_000)?;
        writer.delete_term(Term::from_field_text(self.fields.doc_id, doc_id));
        writer.commit()?;
        Ok(())
    }

    /// Top-k chunk hits for `query`. BM25 score from tantivy, normalized to
    /// 0.0–1.0 via per-batch max-scaling. Sentinels are excluded.
    ///
    /// `filter` is applied **post-retrieval** in v0.2: tantivy returns up to
    /// `top_k * FILTER_OVERFETCH` candidates, then `metadata` and `created_at`
    /// are checked in Rust. This avoids per-key tantivy field declarations
    /// while staying correct for low-selectivity filters. Highly selective
    /// filters on large corpora may want query-time filtering — TODO(v0.4).
    pub fn search(
        &self,
        query: &str,
        top_k: usize,
        filter: Option<&SearchFilter>,
    ) -> Result<Vec<ChunkHit>> {
        self.search_excluding(query, top_k, filter, &HashSet::new())
    }

    /// Like [`Self::search`] but also drops chunks whose `doc_id` is in
    /// `exclude_doc_ids`. Used by `msa_search_iterative` to dedup across
    /// rounds of a Memory Interleave session.
    pub fn search_excluding(
        &self,
        query: &str,
        top_k: usize,
        filter: Option<&SearchFilter>,
        exclude_doc_ids: &HashSet<String>,
    ) -> Result<Vec<ChunkHit>> {
        self.search_hybrid(query, top_k, filter, exclude_doc_ids, 1.0, None)
    }

    /// BM25 + optional dense rerank. `dense_alpha` is the BM25 weight in
    /// `[0.0, 1.0]`: `1.0` is BM25-only, `0.0` is dense-only. `query_emb`
    /// must be `Some(&[f32])` whenever `dense_alpha < 1.0`; otherwise the
    /// call is rejected with `MsaError::InvalidQuery`. Chunks indexed
    /// without a stored embedding contribute neutral cosine `0.5`.
    pub fn search_hybrid(
        &self,
        query: &str,
        top_k: usize,
        filter: Option<&SearchFilter>,
        exclude_doc_ids: &HashSet<String>,
        dense_alpha: f32,
        query_emb: Option<&[f32]>,
    ) -> Result<Vec<ChunkHit>> {
        if query.trim().is_empty() {
            return Err(MsaError::InvalidQuery("query is empty".into()));
        }
        if !(0.0..=1.0).contains(&dense_alpha) {
            return Err(MsaError::InvalidQuery(format!(
                "dense_alpha must be in [0.0, 1.0], got {dense_alpha}"
            )));
        }
        let dense_active = dense_alpha < 1.0;
        if dense_active && query_emb.is_none() {
            return Err(MsaError::InvalidQuery(
                "dense_alpha < 1.0 requires a query embedding".into(),
            ));
        }

        self.reader.reload()?;
        let searcher = self.reader.searcher();
        let qp = QueryParser::for_index(&self.index, vec![self.fields.chunk_text]);
        let parsed = qp
            .parse_query(query)
            .map_err(|e| MsaError::InvalidQuery(e.to_string()))?;

        // Overfetch when a filter, exclude list, or dense rerank is active
        // so post-processing still has a decent chance of returning top_k.
        const POSTPROCESS_OVERFETCH: usize = 4;
        const MAX_OVERFETCH: usize = 1024;
        let needs_overfetch = filter.is_some() || !exclude_doc_ids.is_empty() || dense_active;
        let fetch_limit = if needs_overfetch {
            (top_k.saturating_mul(POSTPROCESS_OVERFETCH)).min(MAX_OVERFETCH)
        } else {
            top_k
        }
        .max(1);

        let top = searcher.search(&parsed, &TopDocs::with_limit(fetch_limit))?;
        let max_raw = top
            .iter()
            .map(|(s, _)| *s)
            .fold(f32::NEG_INFINITY, f32::max);

        // First pass: collect candidates after filter + exclude. Score is
        // BM25-normalized; dense rerank happens in a second pass below.
        let mut candidates: Vec<ChunkHit> = Vec::with_capacity(top.len());
        let mut chunk_embeddings: Vec<Vec<f32>> = Vec::with_capacity(top.len());
        for (raw_score, addr) in top {
            let stored: TantivyDocument = searcher.doc(addr)?;
            let chunk_idx = first_u64(&stored, self.fields.chunk_idx).unwrap_or(0) as usize;
            if chunk_idx as u64 == SENTINEL_CHUNK_IDX as usize as u64 {
                continue;
            }
            let doc_id = first_text(&stored, self.fields.doc_id).unwrap_or_default();
            if exclude_doc_ids.contains(&doc_id) {
                continue;
            }
            let snippet = first_text(&stored, self.fields.chunk_text).unwrap_or_default();
            let cs = first_u64(&stored, self.fields.char_start).unwrap_or(0) as usize;
            let ce = first_u64(&stored, self.fields.char_end).unwrap_or(0) as usize;
            let metadata = first_text(&stored, self.fields.metadata)
                .and_then(|s| serde_json::from_str::<HashMap<String, serde_json::Value>>(&s).ok())
                .unwrap_or_default();
            let created_unix = first_i64(&stored, self.fields.created_at).unwrap_or(0);
            if let Some(f) = filter {
                if !filter_matches(f, &metadata, created_unix) {
                    continue;
                }
            }

            let normalized = if max_raw > 0.0 {
                raw_score / max_raw
            } else {
                0.0
            };
            let cached_emb = if dense_active {
                first_bytes(&stored, self.fields.embedding)
                    .map(|b| unpack_f32(&b))
                    .unwrap_or_default()
            } else {
                Vec::new()
            };

            candidates.push(ChunkHit {
                doc_id,
                chunk_idx,
                score: normalized,
                raw_score,
                snippet,
                char_offset: (cs, ce),
                metadata,
            });
            chunk_embeddings.push(cached_emb);
        }

        if dense_active {
            let q = query_emb.expect("query_emb required when dense_active");
            for (hit, emb) in candidates.iter_mut().zip(chunk_embeddings.iter()) {
                let cos = if emb.is_empty() {
                    0.5
                } else {
                    cosine_unit(q, emb)
                };
                hit.score = dense_alpha * hit.score + (1.0 - dense_alpha) * cos;
            }
            candidates.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        candidates.truncate(top_k);
        Ok(candidates)
    }

    /// Fetch the full original text of a document via the sentinel record.
    pub fn fetch_doc(&self, doc_id: &str) -> Result<Document> {
        self.reader.reload()?;
        let searcher = self.reader.searcher();

        // Use a term query on doc_id; iterate stored docs to find the sentinel.
        let term = Term::from_field_text(self.fields.doc_id, doc_id);
        let q = tantivy::query::TermQuery::new(term, tantivy::schema::IndexRecordOption::Basic);
        let top = searcher.search(&q, &TopDocs::with_limit(1024))?;

        for (_, addr) in top {
            let stored: TantivyDocument = searcher.doc(addr)?;
            let is_sentinel = first_u64(&stored, self.fields.is_sentinel).unwrap_or(0);
            if is_sentinel == 1 {
                let full_text = first_text(&stored, self.fields.full_text).unwrap_or_default();
                let metadata = first_text(&stored, self.fields.metadata)
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default();
                let created_unix = first_i64(&stored, self.fields.created_at).unwrap_or(0);
                let created_at =
                    DateTime::<Utc>::from_timestamp(created_unix, 0).unwrap_or_else(Utc::now);
                return Ok(Document {
                    id: doc_id.to_string(),
                    text: full_text,
                    metadata,
                    created_at,
                });
            }
        }
        Err(MsaError::UnknownDocument {
            collection: self.name.clone(),
            doc_id: doc_id.to_string(),
        })
    }

    pub fn stats(&self) -> Result<MsaStats> {
        self.reader.reload()?;
        let searcher = self.reader.searcher();
        let total = searcher.num_docs();
        // Approximate: subtract sentinels by scanning is_sentinel; for v0.2
        // we just expose total tantivy docs minus an estimate. The exact split
        // is computed on demand. TODO(v0.3): keep counters out-of-band.
        let disk_bytes = dir_size(&self.path).unwrap_or(0);
        Ok(MsaStats {
            collection: self.name.clone(),
            num_documents: 0, // TODO(v0.2): count distinct doc_id
            num_chunks: total,
            total_tokens: 0, // TODO(v0.2): sum chunk token_counts
            disk_bytes,
        })
    }
}

fn first_text(doc: &TantivyDocument, field: Field) -> Option<String> {
    match doc.get_first(field)? {
        OwnedValue::Str(s) => Some(s.clone()),
        _ => None,
    }
}

fn first_u64(doc: &TantivyDocument, field: Field) -> Option<u64> {
    match doc.get_first(field)? {
        OwnedValue::U64(n) => Some(*n),
        _ => None,
    }
}

fn first_i64(doc: &TantivyDocument, field: Field) -> Option<i64> {
    match doc.get_first(field)? {
        OwnedValue::I64(n) => Some(*n),
        _ => None,
    }
}

fn first_bytes(doc: &TantivyDocument, field: Field) -> Option<Vec<u8>> {
    match doc.get_first(field)? {
        OwnedValue::Bytes(b) => Some(b.clone()),
        _ => None,
    }
}

/// Apply a `SearchFilter` against a chunk's metadata + `created_at` (unix s).
fn filter_matches(
    f: &SearchFilter,
    metadata: &HashMap<String, serde_json::Value>,
    created_unix: i64,
) -> bool {
    for (k, expected) in &f.where_eq {
        match metadata.get(k) {
            Some(actual) if actual == expected => {}
            _ => return false,
        }
    }
    for (k, allowed) in &f.where_in {
        match metadata.get(k) {
            Some(actual) if allowed.iter().any(|v| v == actual) => {}
            _ => return false,
        }
    }
    if let Some(after) = f.created_after {
        if created_unix < after.timestamp() {
            return false;
        }
    }
    if let Some(before) = f.created_before {
        if created_unix > before.timestamp() {
            return false;
        }
    }
    true
}

fn dir_size(path: &std::path::Path) -> Result<u64> {
    let mut total = 0u64;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_file() {
            total += meta.len();
        } else if meta.is_dir() {
            total += dir_size(&entry.path())?;
        }
    }
    Ok(total)
}

/// Process-wide registry of open indices, keyed by collection name.
#[derive(Default)]
pub struct CollectionRegistry {
    inner: std::sync::Mutex<HashMap<String, Arc<MsaIndex>>>,
}

impl CollectionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_or_create(
        &self,
        name: &str,
        base_dir: &std::path::Path,
        chunking: &ChunkConfig,
    ) -> Result<Arc<MsaIndex>> {
        let mut guard = self.inner.lock().expect("registry poisoned");
        if let Some(idx) = guard.get(name) {
            return Ok(idx.clone());
        }
        let path = base_dir.join(name);
        let idx = Arc::new(MsaIndex::open(name.to_string(), path, chunking.clone())?);
        guard.insert(name.to_string(), idx.clone());
        Ok(idx)
    }

    pub fn list(&self) -> Vec<String> {
        self.inner
            .lock()
            .expect("registry poisoned")
            .keys()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh_index() -> (TempDir, MsaIndex) {
        let dir = tempfile::tempdir().unwrap();
        let idx = MsaIndex::open(
            "test".into(),
            dir.path().to_path_buf(),
            ChunkConfig::default(),
        )
        .unwrap();
        (dir, idx)
    }

    #[test]
    fn index_and_search_basic() {
        let (_d, idx) = fresh_index();
        idx.index_document(
            &Document {
                id: "d1".into(),
                text: "il gatto nero dorme sul divano rosso".into(),
                metadata: Default::default(),
                created_at: Utc::now(),
            },
            None,
        )
        .unwrap();
        idx.index_document(
            &Document {
                id: "d2".into(),
                text: "configurazione XML namespace schema validation parser".into(),
                metadata: Default::default(),
                created_at: Utc::now(),
            },
            None,
        )
        .unwrap();

        let hits = idx.search("gatto", 5, None).unwrap();
        assert!(!hits.is_empty(), "expected hits for 'gatto'");
        assert_eq!(hits[0].doc_id, "d1");
        assert!(hits[0].score > 0.0);
        assert!(hits[0].score <= 1.0 + f32::EPSILON);
    }

    #[test]
    fn fetch_doc_returns_full_text() {
        let (_d, idx) = fresh_index();
        let original = "alpha bravo charlie delta echo foxtrot golf hotel";
        idx.index_document(
            &Document {
                id: "d1".into(),
                text: original.into(),
                metadata: Default::default(),
                created_at: Utc::now(),
            },
            None,
        )
        .unwrap();
        let fetched = idx.fetch_doc("d1").unwrap();
        assert_eq!(fetched.text, original);
    }

    #[test]
    fn search_filter_where_eq_drops_non_matching() {
        let (_d, idx) = fresh_index();
        let mut meta_a = HashMap::new();
        meta_a.insert("kind".to_string(), serde_json::json!("turn"));
        let mut meta_b = HashMap::new();
        meta_b.insert("kind".to_string(), serde_json::json!("loop_runtime"));

        idx.index_document(
            &Document {
                id: "a".into(),
                text: "alpha gatto bravo".into(),
                metadata: meta_a,
                created_at: Utc::now(),
            },
            None,
        )
        .unwrap();
        idx.index_document(
            &Document {
                id: "b".into(),
                text: "alpha gatto bravo".into(), // identical text
                metadata: meta_b,
                created_at: Utc::now(),
            },
            None,
        )
        .unwrap();

        let mut filter = SearchFilter::default();
        filter
            .where_eq
            .insert("kind".to_string(), serde_json::json!("turn"));

        let hits = idx.search("gatto", 5, Some(&filter)).unwrap();
        assert!(!hits.is_empty(), "expected at least one hit");
        assert!(
            hits.iter().all(|h| h.doc_id == "a"),
            "filter should keep only doc 'a', got: {:?}",
            hits.iter().map(|h| h.doc_id.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn search_filter_where_in_accepts_any() {
        let (_d, idx) = fresh_index();
        let kinds = ["turn", "loop_runtime", "loop_config"];
        for (i, kind) in kinds.iter().enumerate() {
            let mut meta = HashMap::new();
            meta.insert("kind".to_string(), serde_json::json!(kind));
            idx.index_document(
                &Document {
                    id: format!("d{i}"),
                    text: "alpha gatto bravo".into(),
                    metadata: meta,
                    created_at: Utc::now(),
                },
                None,
            )
            .unwrap();
        }

        let mut filter = SearchFilter::default();
        filter.where_in.insert(
            "kind".to_string(),
            vec![serde_json::json!("turn"), serde_json::json!("loop_config")],
        );

        let hits = idx.search("gatto", 5, Some(&filter)).unwrap();
        let ids: Vec<_> = hits.iter().map(|h| h.doc_id.clone()).collect();
        assert!(
            ids.contains(&"d0".to_string()),
            "expected d0 (turn) in {ids:?}"
        );
        assert!(
            ids.contains(&"d2".to_string()),
            "expected d2 (loop_config) in {ids:?}"
        );
        assert!(
            !ids.contains(&"d1".to_string()),
            "did not expect d1 (loop_runtime) in {ids:?}"
        );
    }

    #[test]
    fn search_excluding_drops_listed_doc_ids() {
        let (_d, idx) = fresh_index();
        for i in 0..5 {
            idx.index_document(
                &Document {
                    id: format!("d{i}"),
                    text: "alpha gatto bravo charlie".into(),
                    metadata: Default::default(),
                    created_at: Utc::now(),
                },
                None,
            )
            .unwrap();
        }

        let mut excludes = HashSet::new();
        excludes.insert("d0".to_string());
        excludes.insert("d2".to_string());

        let hits = idx.search_excluding("gatto", 5, None, &excludes).unwrap();
        let ids: Vec<_> = hits.iter().map(|h| h.doc_id.clone()).collect();
        assert!(
            !ids.contains(&"d0".into()),
            "d0 should be excluded, got {ids:?}"
        );
        assert!(
            !ids.contains(&"d2".into()),
            "d2 should be excluded, got {ids:?}"
        );
        // The remaining three docs should all be reachable.
        assert!(ids.iter().any(|id| id == "d1"));
        assert!(ids.iter().any(|id| id == "d3"));
        assert!(ids.iter().any(|id| id == "d4"));
    }

    #[test]
    fn search_hybrid_combines_bm25_and_dense() {
        use crate::embeddings::testing::MockScorer;
        let (_d, idx) = fresh_index();
        let scorer = MockScorer::new(8);

        // Both docs share the BM25 token "alpha", but only doc "first"
        // matches the dense embedding (mock keys on the first whitespace
        // word). With dense_alpha < 1 the rerank should prefer "first".
        idx.index_document(
            &Document {
                id: "first".into(),
                text: "first alpha bravo charlie".into(),
                metadata: Default::default(),
                created_at: Utc::now(),
            },
            Some(&scorer),
        )
        .unwrap();
        idx.index_document(
            &Document {
                id: "delta".into(),
                text: "delta alpha bravo charlie".into(),
                metadata: Default::default(),
                created_at: Utc::now(),
            },
            Some(&scorer),
        )
        .unwrap();

        let q_emb = scorer.embed("first").unwrap();
        let hits = idx
            .search_hybrid("alpha", 5, None, &HashSet::new(), 0.2, Some(&q_emb))
            .unwrap();
        assert!(!hits.is_empty());
        assert_eq!(
            hits[0].doc_id,
            "first",
            "dense rerank should rank 'first' above 'delta' (got {:?})",
            hits.iter()
                .map(|h| (h.doc_id.clone(), h.score))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn search_hybrid_rejects_alpha_lt_one_without_query_embedding() {
        let (_d, idx) = fresh_index();
        idx.index_document(
            &Document {
                id: "x".into(),
                text: "alpha".into(),
                metadata: Default::default(),
                created_at: Utc::now(),
            },
            None,
        )
        .unwrap();
        let err = idx
            .search_hybrid("alpha", 5, None, &HashSet::new(), 0.5, None)
            .unwrap_err();
        match err {
            MsaError::InvalidQuery(_) => {}
            other => panic!("expected InvalidQuery, got {other:?}"),
        }
    }

    #[test]
    fn search_hybrid_falls_back_to_neutral_cosine_when_chunk_has_no_embedding() {
        use crate::embeddings::testing::MockScorer;
        let (_d, idx) = fresh_index();
        let scorer = MockScorer::new(8);
        // Indexed without scorer → no chunk embedding stored.
        idx.index_document(
            &Document {
                id: "x".into(),
                text: "alpha bravo".into(),
                metadata: Default::default(),
                created_at: Utc::now(),
            },
            None,
        )
        .unwrap();
        let q_emb = scorer.embed("alpha").unwrap();
        // Should not panic and should still return a hit.
        let hits = idx
            .search_hybrid("alpha", 5, None, &HashSet::new(), 0.5, Some(&q_emb))
            .unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn delete_removes_chunks() {
        let (_d, idx) = fresh_index();
        idx.index_document(
            &Document {
                id: "d1".into(),
                text: "termine specifico unico".into(),
                metadata: Default::default(),
                created_at: Utc::now(),
            },
            None,
        )
        .unwrap();
        assert!(!idx.search("specifico", 5, None).unwrap().is_empty());
        idx.delete_document("d1").unwrap();
        assert!(idx.search("specifico", 5, None).unwrap().is_empty());
    }
}
