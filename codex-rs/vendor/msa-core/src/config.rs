use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{MsaError, Result};
use crate::schema::ChunkConfig;

/// Runtime configuration. Loaded from `MCP_MSA_CONFIG` (TOML) or filled with
/// defaults rooted at `~/.local/state/mcp-msa-rs/`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsaConfig {
    pub storage: StorageConfig,
    #[serde(default)]
    pub chunking: ChunkConfig,
    /// Optional dense scorer. Has effect only when the binary is built
    /// with `--features embeddings`. With the feature off the section is
    /// accepted but ignored, with a warning logged at startup.
    #[serde(default)]
    pub embeddings: Option<EmbeddingsConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingsConfig {
    /// Backend identifier. Currently only `"ollama"` is implemented.
    pub backend: String,
    /// Base URL of the embedding service (e.g. `http://127.0.0.1:11434`).
    pub url: String,
    /// Model name passed to the backend (e.g. `nomic-embed-text`).
    pub model: String,
    /// Dimension reported by the model. Used to validate cached embeddings.
    pub dim: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Base directory: each collection lives under `<storage_dir>/<name>/`.
    pub storage_dir: PathBuf,
}

impl Default for MsaConfig {
    fn default() -> Self {
        Self {
            storage: StorageConfig {
                storage_dir: default_storage_dir(),
            },
            chunking: ChunkConfig::default(),
            embeddings: None,
        }
    }
}

fn default_storage_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local/state/mcp-msa-rs")
    } else {
        PathBuf::from(".mcp-msa-rs")
    }
}

impl MsaConfig {
    /// Load config from `MCP_MSA_CONFIG` env path if set, otherwise defaults.
    pub fn load() -> Result<Self> {
        match std::env::var("MCP_MSA_CONFIG") {
            Ok(path) => {
                let raw = std::fs::read_to_string(&path)
                    .map_err(|e| MsaError::Config(format!("read {path}: {e}")))?;
                toml::from_str(&raw).map_err(|e| MsaError::Config(format!("parse {path}: {e}")))
            }
            Err(_) => Ok(Self::default()),
        }
    }

    pub fn collection_path(&self, name: &str) -> PathBuf {
        self.storage.storage_dir.join(sanitize_collection(name))
    }
}

/// Replace path separators with underscore so collection names cannot escape
/// the storage root.
fn sanitize_collection(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | '.' if c == '.' => '_',
            '/' | '\\' => '_',
            _ => c,
        })
        .collect()
}
