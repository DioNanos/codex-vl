use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use msa_core::config::MsaConfig;
use msa_core::index::CollectionRegistry;
use msa_core::index::MsaIndex;
use msa_core::interleave::DEFAULT_LOW_GAIN_THRESHOLD;
use msa_core::interleave::DedupMode;
use msa_core::interleave::InterleaveParams;
use msa_core::interleave::InterleaveStateV1;
use msa_core::interleave::RoundResponse;
use msa_core::interleave::run_round;
use msa_core::schema::ChunkConfig;
use msa_core::schema::Document;

use super::super::model::VivlingWorkMemoryEntry;

/// Tier knobs for the Vivling brain-prompt recall (design 2026-06-05, v2).
/// Conservative: the brain prompt is a much smaller surface than the MCP
/// server tier (16000c in the A5 bench). All bounds re-clamped by
/// [`InterleaveParams::clamped`].
const RECALL_TOP_K: usize = 5;
const RECALL_FETCH_TOP_N: usize = 3;
const RECALL_MAX_CHARS_PER_DOC: usize = 1500;
const RECALL_MAX_TOTAL_CHARS: usize = 6000;
const RECALL_MAX_ROUNDS: u32 = 8;
/// Detail cap for rich capsule indexing (design "capsule ricche": 800, not
/// the 1500 per-doc injection cap — start conservative).
const RICH_DETAIL_MAX_CHARS: usize = 800;

#[derive(Clone)]
pub(crate) struct VivlingMsa {
    registry: Arc<CollectionRegistry>,
    storage_dir: PathBuf,
    chunk_config: ChunkConfig,
    /// Ephemeral, in-process Memory Interleave state per vivling (design v2:
    /// NEVER serialized into `VivlingState` — no schema change, no backfill,
    /// no persistence boundary). Cross-tick dedup holds for the process
    /// lifetime; a restart starts fresh by construction.
    interleave_state: Arc<Mutex<HashMap<String, InterleaveStateV1>>>,
}

impl std::fmt::Debug for VivlingMsa {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VivlingMsa")
            .field("storage_dir", &self.storage_dir)
            .field("chunk_config", &self.chunk_config)
            .finish_non_exhaustive()
    }
}

impl VivlingMsa {
    pub(crate) fn open() -> Option<Self> {
        let cfg = match MsaConfig::load() {
            Ok(cfg) => cfg,
            Err(err) => {
                tracing::warn!(
                    target: "vivling::msa",
                    "MsaConfig::load failed, vivling retrieval disabled: {err}"
                );
                return None;
            }
        };
        let storage_dir = cfg.storage.storage_dir.clone();
        match Self::open_from_parts(cfg.storage.storage_dir, cfg.chunking) {
            Some(this) => {
                tracing::info!(
                    target: "vivling::msa",
                    "vivling msa storage opened at {}",
                    storage_dir.display()
                );
                Some(this)
            }
            None => {
                tracing::warn!(
                    target: "vivling::msa",
                    "vivling msa storage unavailable at {} (mkdir failed), retrieval disabled",
                    storage_dir.display()
                );
                None
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn open_for_tests(storage_dir: &std::path::Path) -> Self {
        Self::open_from_parts(storage_dir.to_path_buf(), ChunkConfig::default())
            .expect("test MSA storage should open")
    }

    fn open_from_parts(storage_dir: PathBuf, chunk_config: ChunkConfig) -> Option<Self> {
        std::fs::create_dir_all(&storage_dir).ok()?;
        Some(Self {
            registry: Arc::new(CollectionRegistry::new()),
            storage_dir,
            chunk_config,
            interleave_state: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub(crate) fn collection_for(&self, vivling_id: &str) -> Option<Arc<MsaIndex>> {
        let name = format!("vivling::{vivling_id}");
        match self
            .registry
            .open_or_create(&name, &self.storage_dir, &self.chunk_config)
        {
            Ok(idx) => Some(idx),
            Err(err) => {
                tracing::warn!(
                    target: "vivling::msa",
                    "open_or_create({name}) failed: {err}"
                );
                None
            }
        }
    }

    /// Original-text recall for the brain prompt (MSA injection lever,
    /// A5 gate: injection reaches the retrieval ceiling, snippets cannot).
    ///
    /// Runs one Memory Interleave round against the vivling's collection and
    /// renders the injected documents as a prompt section. The interleave
    /// state is managed entirely inside this adapter (ephemeral, see field
    /// doc): the caller neither sees nor mutates it, and the brain prompt
    /// composition path keeps its `&VivlingState` signature.
    ///
    /// Reset policy (design v2): if the stored state is saturated (round or
    /// char budget exhausted → `run_round` pre-stops with no injected docs),
    /// the state is reset and ONE fresh round runs in the same tick, so
    /// injection can never be permanently disabled. Returns `None` on any
    /// outcome without injected docs — the caller falls back to the legacy
    /// snippet/recent-work section and the tick always completes.
    pub(crate) fn recall_section(&self, vivling_id: &str, payload: &str) -> Option<String> {
        let idx = self.collection_for(vivling_id)?;
        let params = InterleaveParams::clamped(
            RECALL_TOP_K,
            RECALL_FETCH_TOP_N,
            RECALL_MAX_CHARS_PER_DOC,
            RECALL_MAX_TOTAL_CHARS,
            RECALL_MAX_ROUNDS,
            DEFAULT_LOW_GAIN_THRESHOLD,
            DedupMode::Id,
        );

        let state = {
            let map = self.interleave_state.lock().ok()?;
            map.get(vivling_id).cloned().unwrap_or_default()
        };

        let mut response = self.recall_round(&idx, vivling_id, payload, &state, &params)?;
        if response.injected_docs.is_empty() && response.stop_hints.hard_stop {
            // Saturation: reset and run one fresh round in the same tick.
            tracing::debug!(
                target: "vivling::msa",
                "interleave state saturated for vivling::{vivling_id} (round {}), resetting",
                response.state.round
            );
            response = self.recall_round(
                &idx,
                vivling_id,
                payload,
                &InterleaveStateV1::fresh(),
                &params,
            )?;
        }

        if let Ok(mut map) = self.interleave_state.lock() {
            map.insert(vivling_id.to_string(), response.state.clone());
        }

        if response.injected_docs.is_empty() {
            return None;
        }

        let mut lines = vec!["Relevant memory (original text):".to_string()];
        for doc in &response.injected_docs {
            let kind = doc
                .metadata
                .get("kind")
                .and_then(|value| value.as_str())
                .unwrap_or("?");
            // Serving-side gate twin of the ingest gate (F1): archives
            // indexed BEFORE the gate shipped still hold bookkeeping docs —
            // never serve them, on any install, without requiring a rebuild.
            if crate::vivling::model::constants::BOOKKEEPING_KINDS.contains(&kind) {
                continue;
            }
            let archetype = doc
                .metadata
                .get("archetype")
                .and_then(|value| value.as_str())
                .unwrap_or("?");
            let marker = if doc.truncated { " [truncated]" } else { "" };
            lines.push(format!("- {kind} [{archetype}]{marker}: {}", doc.text));
        }
        if lines.len() == 1 {
            return None; // every injected doc was legacy bookkeeping
        }
        Some(lines.join("\n"))
    }

    fn recall_round(
        &self,
        idx: &MsaIndex,
        vivling_id: &str,
        payload: &str,
        state: &InterleaveStateV1,
        params: &InterleaveParams,
    ) -> Option<RoundResponse> {
        // dense_alpha 1.0 = BM25-only, explicit: no dense path, no embeddings.
        match run_round(idx, payload, state, params, None, 1.0, None) {
            Ok(response) => Some(response),
            Err(err) => {
                tracing::warn!(
                    target: "vivling::msa",
                    "recall run_round failed for vivling::{vivling_id}: {err}"
                );
                None
            }
        }
    }

    /// Drop the ephemeral interleave state for one vivling. Hook for work
    /// memory rotation (design v2 reset trigger c); currently exercised by
    /// tests — capsules only accumulate today.
    #[allow(dead_code)]
    pub(crate) fn reset_interleave_state(&self, vivling_id: &str) {
        if let Ok(mut map) = self.interleave_state.lock() {
            map.remove(vivling_id);
        }
    }

    pub(crate) fn index_capsule(&self, vivling_id: &str, capsule: &VivlingWorkMemoryEntry) {
        self.index_capsule_rich(vivling_id, capsule, None);
    }

    /// Index a capsule, optionally enriching `Document.text` with a bounded,
    /// sanitized detail section (design "capsule ricche" 2026-06-05).
    ///
    /// Policy (adapter-owned, the engine in `msa_core::enrich` is
    /// domain-neutral): only `turn` capsules with a non-low-signal source get
    /// the rich layout; everything else keeps today's summary-only document.
    /// The rich text is an INDEX artifact only — nothing here touches the
    /// capsule or the serialized Vivling state.
    pub(crate) fn index_capsule_rich(
        &self,
        vivling_id: &str,
        capsule: &VivlingWorkMemoryEntry,
        rich_source: Option<&str>,
    ) {
        use msa_core::enrich;

        // Ingest gate (live audit 2026-06-07, F1): operational bookkeeping
        // stays in the local working memory but is NOT indexed into the
        // long-term MSA archive — 78% of a 6-week-old vivling's index was
        // this noise, crowding knowledge out of recall_section. Denylist
        // (not allowlist) so future knowledge kinds flow by default. This
        // single choke point also gates the setup backfill (F7) and the
        // lineage echo path.
        if crate::vivling::model::constants::BOOKKEEPING_KINDS
            .contains(&capsule.kind.as_str())
        {
            return;
        }

        let Some(idx) = self.collection_for(vivling_id) else {
            return;
        };
        let doc_id = format!(
            "cap::{}::{}",
            capsule.created_at.timestamp_nanos_opt().unwrap_or(0),
            capsule.kind,
        );

        let detail = rich_source
            .filter(|_| capsule.kind == "turn")
            .map(enrich::sanitize_detail)
            .filter(|d| !enrich::is_low_signal_detail(d));
        let composed = match detail.as_deref() {
            Some(d) => enrich::compose_rich_text(&capsule.summary, d, RICH_DETAIL_MAX_CHARS),
            None => enrich::compose_rich_text(&capsule.summary, "", RICH_DETAIL_MAX_CHARS),
        };
        let rich = composed.detail_chars > 0;

        let mut metadata: HashMap<String, serde_json::Value> = HashMap::new();
        metadata.insert("kind".into(), serde_json::json!(capsule.kind));
        metadata.insert(
            "archetype".into(),
            serde_json::json!(capsule.archetype.label()),
        );
        metadata.insert("weight".into(), serde_json::json!(capsule.weight));
        metadata.insert(
            "day".into(),
            serde_json::json!(capsule.created_at.format("%F").to_string()),
        );
        metadata.insert(
            enrich::METADATA_KEY_INDEX_TEXT_VERSION.into(),
            serde_json::json!(enrich::INDEX_TEXT_VERSION),
        );
        metadata.insert(enrich::METADATA_KEY_RICH.into(), serde_json::json!(rich));
        if rich {
            metadata.insert(
                enrich::METADATA_KEY_DETAIL_CHARS.into(),
                serde_json::json!(composed.detail_chars),
            );
            metadata.insert(
                enrich::METADATA_KEY_SOURCE.into(),
                serde_json::json!("turn_summary_sanitized"),
            );
        }
        let doc = Document {
            id: doc_id,
            text: composed.text,
            metadata,
            created_at: capsule.created_at,
        };
        match idx.index_document(&doc, None) {
            Ok(_) => {
                tracing::info!(
                    target: "vivling::msa",
                    "indexed capsule for vivling::{vivling_id} (kind={}, weight={})",
                    capsule.kind,
                    capsule.weight
                );
            }
            Err(err) => {
                tracing::warn!(
                    target: "vivling::msa",
                    "index_capsule failed for vivling::{vivling_id} (kind={}): {err}",
                    capsule.kind
                );
            }
        }
    }
}
