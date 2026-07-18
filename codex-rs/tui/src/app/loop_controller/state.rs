//! codex-vl loop_controller: free helpers (timing + error wrapping).
//!
//! `loop_state_runtime` remains a `pub(super) async fn` on `impl App`
//! in `mod.rs` because several App methods share that call pattern. It
//! clones the process-owned state handle instead of reopening and
//! migrating SQLite from a consumer path. Only the byte-pure timing and
//! error helpers remain here.

pub(super) fn loop_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub(super) fn loop_state_error(err: anyhow::Error) -> color_eyre::Report {
    color_eyre::eyre::eyre!("{err}")
}
