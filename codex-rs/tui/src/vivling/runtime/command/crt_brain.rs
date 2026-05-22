//! Memory V2 Step 12.B.D.3 — `/vivling crt-brain` command handlers.
//!
//! Three mutators (`On`, `Off`, `Default`) flip
//! `state.crt_brain_mode` and persist via `update_existing`. `Show`
//! is a pure read: it formats the current mode together with today's
//! LLM call counters (Chat / Assist / LoopTick / Expression plus the
//! skip breakdown — Throttle / Dedup / Budget / OptOut / Failures)
//! so the user can spot-check the cost surface in one glance.
//!
//! Bond: this command never spends an LLM call, never opens a panel,
//! and never sends data to a remote. The toggle path matches the
//! existing `/vivling brain on|off` semantics — fast, local-only.

use super::super::expression;
use super::*;
use codex_vivling_core::model::VivlingExpressionMode;

impl Vivling {
    pub(crate) fn crt_brain_show(&self) -> Result<String, String> {
        let state = self
            .state
            .as_ref()
            .ok_or_else(|| "No active Vivling. Run `/vivling hatch` first.".to_string())?;
        Ok(expression::format_crt_brain_status(state))
    }

    pub(crate) fn crt_brain_set(&mut self, mode: VivlingExpressionMode) -> Result<String, String> {
        self.update_existing(|state| {
            state.crt_brain_mode = mode;
            format_set_message(mode)
        })
    }
}

fn format_set_message(mode: VivlingExpressionMode) -> String {
    match mode {
        VivlingExpressionMode::Default => {
            "CRT brain mode: default (stage-driven — Adult/Juvenile run, Baby rare-event only)."
                .to_string()
        }
        VivlingExpressionMode::On => {
            "CRT brain mode: on (Expression channel forced on regardless of stage).".to_string()
        }
        VivlingExpressionMode::Off => {
            "CRT brain mode: off (Expression channel muted; CRT falls back to template chain)."
                .to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vivling::model::SeedIdentity;
    use crate::vivling::model::VivlingState;
    use codex_vivling_core::model::ADULT_LEVEL;
    use codex_vivling_core::model::Stage;

    fn fixture_state() -> VivlingState {
        let mut s = VivlingState::new(SeedIdentity {
            value: "step-12bd3-fixture".to_string(),
            install_id: None,
        });
        s.level = ADULT_LEVEL;
        assert_eq!(s.stage(), Stage::Adult);
        s
    }

    #[test]
    fn format_set_message_distinguishes_three_modes() {
        let default_msg = format_set_message(VivlingExpressionMode::Default);
        let on_msg = format_set_message(VivlingExpressionMode::On);
        let off_msg = format_set_message(VivlingExpressionMode::Off);
        assert_ne!(default_msg, on_msg);
        assert_ne!(default_msg, off_msg);
        assert_ne!(on_msg, off_msg);
        assert!(default_msg.contains("default"));
        assert!(on_msg.contains("on"));
        assert!(off_msg.contains("off"));
    }

    #[test]
    fn format_crt_brain_status_includes_mode_and_counters() {
        let mut state = fixture_state();
        state.crt_brain_mode = VivlingExpressionMode::On;
        state.daily_llm_day_key = "2026-05-21".to_string();
        state.daily_llm_call_count = 7;
        state.daily_llm_chat_calls = 2;
        state.daily_llm_assist_calls = 1;
        state.daily_llm_loop_tick_calls = 1;
        state.daily_llm_expression_calls = 3;
        state.daily_llm_throttle_skips = 4;
        state.daily_llm_dedup_skips = 0;
        state.daily_llm_budget_skips = 1;
        state.daily_llm_optout_skips = 0;
        state.daily_llm_failure_count = 2;

        let text = expression::format_crt_brain_status(&state);
        assert!(text.contains("on"), "must show mode: {text}");
        assert!(text.contains("2026-05-21"), "must show day key: {text}");
        assert!(text.contains("chat 2"), "must show chat count: {text}");
        assert!(text.contains("assist 1"), "must show assist count: {text}");
        assert!(text.contains("loop 1"), "must show loop count: {text}");
        assert!(
            text.contains("expression 3"),
            "must show expression count: {text}"
        );
        assert!(
            text.contains("throttle 4"),
            "must show throttle skips: {text}"
        );
        assert!(text.contains("budget 1"), "must show budget skips: {text}");
        assert!(
            text.contains("failures 2"),
            "must show failure count: {text}"
        );
    }

    #[test]
    fn format_crt_brain_status_renders_zero_counters_cleanly() {
        let state = fixture_state();
        let text = expression::format_crt_brain_status(&state);
        // Brand-new Vivling: every counter at zero. The renderer must
        // still produce something meaningful, not crash or print "n/a".
        assert!(
            text.contains("default"),
            "fresh state defaults to mode default: {text}"
        );
        assert!(
            text.contains("chat 0"),
            "zero chat counter must render: {text}"
        );
    }

    #[test]
    fn format_crt_brain_status_shows_budget_remaining() {
        // Step 12.B.K: surface the daily budget cap and remaining
        // headroom inline so users can tell at a glance how close
        // they are to a fallback path (no more silent `/vl` →
        // template degradation discovered only via the fallback
        // marker).
        use codex_vivling_core::model::stage_llm_budget;
        let mut state = fixture_state();
        state.daily_llm_call_count = 17;
        let cap = stage_llm_budget(state.stage());
        let text = expression::format_crt_brain_status(&state);
        assert!(
            text.contains(&format!("total 17/{cap}")),
            "render must show used/cap fraction: {text}"
        );
        assert!(
            text.contains(&format!("({} left)", cap - 17)),
            "render must show remaining headroom: {text}"
        );
    }
}
