//! codex-vl loop_controller: free helpers (timing + error wrapping).
//!
//! `loop_state_runtime` rimane metodo `pub(super) async fn` su `impl App`
//! in `mod.rs` perché il pattern di chiamata esistente è
//! `self.loop_state_runtime().await` da molti metodi App; estrarlo come
//! free fn richiederebbe una refactor di tutti i caller. Qui restano
//! solo i due helper byte-pure.

pub(super) fn loop_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub(super) fn loop_state_error(err: anyhow::Error) -> color_eyre::Report {
    color_eyre::eyre::eyre!("{err}")
}
