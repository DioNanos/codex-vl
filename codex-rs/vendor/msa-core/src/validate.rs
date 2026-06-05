//! Input validation and resource caps. This is the **single** authority for
//! deciding whether a collection name is safe to turn into an on-disk path,
//! and for bounding the size of caller-supplied inputs.
//!
//! Design choice: collection names are **rejected**, not silently sanitized.
//! Sanitizing (`a/b` → `a_b`) collides distinct names onto the same directory;
//! rejecting is unambiguous and prevents path traversal at the source. The
//! registry calls [`validate_collection_name`] before building any path, so a
//! name like `../../etc` or `/abs` never reaches the filesystem.

use crate::error::{MsaError, Result};

/// Max bytes for a collection name. Collection names map to directory names.
pub const MAX_COLLECTION_NAME_LEN: usize = 128;

/// Max bytes for a document id. Stored as a tantivy STRING field, used as a
/// term; not a path, but bounded to avoid pathological terms.
pub const MAX_DOC_ID_LEN: usize = 512;

/// Default hard ceiling for a single document's text, in bytes. A 10 MiB
/// document already produces a large chunk stream; callers indexing bigger
/// blobs should pre-split. Overridable per deployment via config (future).
pub const DEFAULT_MAX_TEXT_BYTES: usize = 10 * 1024 * 1024;

/// Upper bound for `top_k` to keep a single query's memory/IO bounded even if
/// a caller passes something absurd. Requests above this are clamped, not
/// rejected, so retrieval still works.
pub const MAX_TOP_K: usize = 1000;

/// Max bytes for a search query string. The tantivy query parser builds an
/// AST whose size is roughly linear in the query; a pathologically long query
/// is rejected rather than parsed. Generous (64 KiB) so real LLM-written
/// multi-sentence queries always pass.
pub const MAX_QUERY_BYTES: usize = 64 * 1024;

/// Validate a collection name for use as an on-disk directory name.
///
/// Accepts a conservative whitelist: ASCII alphanumerics plus `_`, `-`, and
/// `:`. The colon is allowed because the Vivling consumer namespaces
/// collections as `vivling::{id}`; it is not a path separator on the unix
/// targets we support. Everything else — `/`, `\`, `.`, whitespace, control
/// characters, non-ASCII — is rejected, which makes `..`, absolute paths, and
/// null-byte tricks impossible by construction.
pub fn validate_collection_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(MsaError::InvalidInput("collection name is empty".into()));
    }
    if name.len() > MAX_COLLECTION_NAME_LEN {
        return Err(MsaError::InvalidInput(format!(
            "collection name too long: {} bytes (max {MAX_COLLECTION_NAME_LEN})",
            name.len()
        )));
    }
    for c in name.chars() {
        let ok = c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':');
        if !ok {
            return Err(MsaError::InvalidInput(format!(
                "collection name contains illegal character {c:?}; \
                 allowed: ASCII alphanumerics, '_', '-', ':'"
            )));
        }
    }
    Ok(())
}

/// Validate a document id: non-empty, bounded length, no NUL.
pub fn validate_doc_id(doc_id: &str) -> Result<()> {
    if doc_id.is_empty() {
        return Err(MsaError::InvalidInput("doc_id is empty".into()));
    }
    if doc_id.len() > MAX_DOC_ID_LEN {
        return Err(MsaError::InvalidInput(format!(
            "doc_id too long: {} bytes (max {MAX_DOC_ID_LEN})",
            doc_id.len()
        )));
    }
    if doc_id.contains('\0') {
        return Err(MsaError::InvalidInput("doc_id contains NUL byte".into()));
    }
    Ok(())
}

/// Validate document text against a byte ceiling.
pub fn validate_text(text: &str, max_bytes: usize) -> Result<()> {
    if text.len() > max_bytes {
        return Err(MsaError::InvalidInput(format!(
            "document text too large: {} bytes (max {max_bytes})",
            text.len()
        )));
    }
    Ok(())
}

/// Validate a search query: non-empty (after trim) and within a byte ceiling.
/// Emptiness is also enforced deeper in the index layer, but rejecting here
/// gives the caller a clean `invalid_params` instead of an internal error.
pub fn validate_query(query: &str) -> Result<()> {
    if query.trim().is_empty() {
        return Err(MsaError::InvalidInput("query is empty".into()));
    }
    if query.len() > MAX_QUERY_BYTES {
        return Err(MsaError::InvalidInput(format!(
            "query too long: {} bytes (max {MAX_QUERY_BYTES})",
            query.len()
        )));
    }
    Ok(())
}

/// Clamp `top_k` into `[1, MAX_TOP_K]`. Zero becomes 1; oversize is capped.
pub fn clamp_top_k(top_k: usize) -> usize {
    top_k.clamp(1, MAX_TOP_K)
}

/// Max bytes for a `source_id` (a sync-scope label, e.g. `"docshub"`).
pub const MAX_SOURCE_ID_LEN: usize = 256;

/// Validate a `source_id`: non-empty, bounded, no NUL. Used to scope sync/
/// manifest operations to one logical source.
pub fn validate_source_id(source_id: &str) -> Result<()> {
    if source_id.is_empty() {
        return Err(MsaError::InvalidInput("source_id is empty".into()));
    }
    if source_id.len() > MAX_SOURCE_ID_LEN {
        return Err(MsaError::InvalidInput(format!(
            "source_id too long: {} bytes (max {MAX_SOURCE_ID_LEN})",
            source_id.len()
        )));
    }
    if source_id.contains('\0') {
        return Err(MsaError::InvalidInput("source_id contains NUL byte".into()));
    }
    Ok(())
}

/// Validate a doc-id filter value (a `doc_id_prefix` or pagination `cursor`):
/// bounded by [`MAX_DOC_ID_LEN`], no NUL. Empty is allowed (an empty prefix
/// matches everything; a `None` cursor is the first page).
pub fn validate_doc_id_filter(value: &str) -> Result<()> {
    if value.len() > MAX_DOC_ID_LEN {
        return Err(MsaError::InvalidInput(format!(
            "doc_id filter too long: {} bytes (max {MAX_DOC_ID_LEN})",
            value.len()
        )));
    }
    if value.contains('\0') {
        return Err(MsaError::InvalidInput("doc_id filter contains NUL byte".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_and_vivling_names() {
        assert!(validate_collection_name("mydocs").is_ok());
        assert!(validate_collection_name("vivling::abc-123").is_ok());
        assert!(validate_collection_name("a_b-c:d").is_ok());
    }

    #[test]
    fn rejects_path_traversal_and_separators() {
        for bad in [
            "",
            "../etc",
            "..",
            "a/b",
            "a\\b",
            "a.b",
            ".",
            "/abs",
            "with space",
            "tab\tname",
            "null\0byte",
            "unicodè",
        ] {
            assert!(
                validate_collection_name(bad).is_err(),
                "expected reject for {bad:?}"
            );
        }
    }

    #[test]
    fn rejects_overlong_name() {
        let long = "a".repeat(MAX_COLLECTION_NAME_LEN + 1);
        assert!(validate_collection_name(&long).is_err());
        let ok = "a".repeat(MAX_COLLECTION_NAME_LEN);
        assert!(validate_collection_name(&ok).is_ok());
    }

    #[test]
    fn doc_id_rules() {
        assert!(validate_doc_id("doc-1").is_ok());
        assert!(validate_doc_id("").is_err());
        assert!(validate_doc_id(&"x".repeat(MAX_DOC_ID_LEN + 1)).is_err());
        assert!(validate_doc_id("a\0b").is_err());
    }

    #[test]
    fn text_cap() {
        assert!(validate_text("small", 100).is_ok());
        assert!(validate_text(&"x".repeat(101), 100).is_err());
    }

    #[test]
    fn top_k_clamp() {
        assert_eq!(clamp_top_k(0), 1);
        assert_eq!(clamp_top_k(16), 16);
        assert_eq!(clamp_top_k(MAX_TOP_K + 5), MAX_TOP_K);
    }

    #[test]
    fn query_rules() {
        assert!(validate_query("quantum computing").is_ok());
        assert!(validate_query("").is_err());
        assert!(validate_query("   ").is_err());
        assert!(validate_query(&"x".repeat(MAX_QUERY_BYTES + 1)).is_err());
    }
}
