//! Sanitization gate for a Rust fork with upstream history.
//!
//! No runtime code: this crate exists only to host the integration tests
//! in `tests/`, which verify that the tracked tree and the fork history
//! carry no traces that would require sanitization before a publication.
//! The rationale mirrors the upstream `published-tree-is-clean` test,
//! adapted to a Rust fork with upstream history.
