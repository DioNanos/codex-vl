use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use msa_core::config::MsaConfig;
use msa_core::index::CollectionRegistry;
use msa_core::index::MsaIndex;
use msa_core::schema::ChunkConfig;
use msa_core::schema::Document;

use super::super::model::VivlingWorkMemoryEntry;

#[derive(Clone)]
pub(crate) struct VivlingMsa {
    registry: Arc<CollectionRegistry>,
    storage_dir: PathBuf,
    chunk_config: ChunkConfig,
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

    pub(crate) fn index_capsule(&self, vivling_id: &str, capsule: &VivlingWorkMemoryEntry) {
        let Some(idx) = self.collection_for(vivling_id) else {
            return;
        };
        let doc_id = format!(
            "cap::{}::{}",
            capsule.created_at.timestamp_nanos_opt().unwrap_or(0),
            capsule.kind,
        );
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
        let doc = Document {
            id: doc_id,
            text: capsule.summary.clone(),
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
