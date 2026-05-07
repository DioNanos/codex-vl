//! mcp-msa-rs — MSA-flavor retrieval engine over tantivy.
//!
//! Public surface:
//! - [`schema`] — `Document`, `ChunkHit`, `SearchFilter`, `MsaStats`
//! - [`chunker`] — word-based chunking with overlap (MSA `P=64` default)
//! - [`index`] — `MsaIndex` wrapping tantivy per collection
//! - [`config`] — runtime configuration loader
//!
//! See the upstream MSA project documentation for design details.

pub mod chunker;
pub mod config;
pub mod embeddings;
pub mod error;
pub mod index;
pub mod schema;
pub mod session;

pub use config::MsaConfig;
pub use error::{MsaError, Result};
pub use index::{CollectionRegistry, MsaIndex};
pub use schema::{ChunkHit, Document, MsaStats, SearchFilter};
