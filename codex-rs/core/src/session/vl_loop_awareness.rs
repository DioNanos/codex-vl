//! codex-vl: loop-awareness developer instructions injected per turn.
//!
//! The main model is told, per turn, whether any `/loop` jobs exist on the
//! current thread and who owns them (main or a Vivling). Keeping this impl
//! block in its own file reduces the upstream surface: `turn_context.rs`
//! only carries a single call site (see `per_turn_context` below).

use super::session::Session;

impl Session {
    pub(crate) async fn build_vl_loop_awareness_developer_instructions(&self) -> Option<String> {
        let state_db = self.services.state_db.as_deref()?;
        let jobs = state_db
            .list_thread_loop_jobs(self.conversation_id)
            .await
            .ok()?;
        if jobs.is_empty() {
            return None;
        }

        let owner = state_db
            .get_thread_loop_owner(self.conversation_id)
            .await
            .ok()?;
        let labels = jobs
            .iter()
            .take(4)
            .map(|job| job.label.clone())
            .collect::<Vec<_>>()
            .join(", ");
        let pending = jobs
            .iter()
            .filter(|job| job.enabled && job.pending_tick)
            .count();
        let owner_summary = match owner.owner_kind.as_str() {
            codex_state::THREAD_LOOP_OWNER_KIND_VIVLING => format!(
                "vivling ({})",
                owner.owner_vivling_id.as_deref().unwrap_or("missing")
            ),
            _ => "main".to_string(),
        };
        let mut lines = vec![
            "[LOOP_AWARENESS]".to_string(),
            "active: true".to_string(),
            format!("owner: {owner_summary}"),
            format!("active_loops: {}", jobs.len()),
            format!("pending_loops: {pending}"),
            format!("labels: {labels}"),
        ];
        if owner.owner_kind == codex_state::THREAD_LOOP_OWNER_KIND_VIVLING {
            lines.push(
                "delegation: loops are currently owned by the Vivling, not by the main session model."
                    .to_string(),
            );
            lines.push(
                "main_model_rule: do not manage, update, or supervise loops unless the user explicitly reassigns ownership to main."
                    .to_string(),
            );
        } else {
            lines.push(
                "main_model_rule: when work needs polling, retries, scheduled follow-up, or recurring monitoring, use the manage_loops tool."
                    .to_string(),
            );
        }
        lines.push(
            "tick_rule: if a loop is still in progress, keep it in progress instead of treating the task as complete."
                .to_string(),
        );
        lines.push("[/LOOP_AWARENESS]".to_string());
        Some(lines.join("\n"))
    }
}
