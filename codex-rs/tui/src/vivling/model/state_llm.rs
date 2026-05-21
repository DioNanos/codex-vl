//! Memory V2 Step 12.B.B — LLM call budget reservation.
//!
//! Pre-dispatch atomic guard for every LLM call a Vivling can spend:
//! `/vl` chat (Juvenile/Adult), `/vivling assist`, loop tick, and the
//! always-on Expression channel (CRT live phrase + proactive).
//!
//! Lives in the TUI tree because the mutator operates on
//! `VivlingState`, which is itself a TUI type (`Step 1.B` kept it out
//! of `codex-vivling-core`). The pure enums and budget constants live
//! in `codex_vivling_core::model::expression`; this module is purely
//! the state-mutating piece.
//!
//! Contract:
//! 1. Reset the daily counters when the UTC day changes (`day_key`
//!    field).
//! 2. Check kind-specific eligibility (`Expression` + `mode == Off`
//!    → `OptOut`, `Chat` + Baby → `NotEligibleStage`).
//! 3. Throttle (`Expression` only): 60s window from
//!    `last_llm_dispatch_at`.
//! 4. Dedup (`Expression` only): when the planner would emit a
//!    prompt whose hash matches the fresh cached entry's hash, the
//!    reservation is refused — no new signal to spend on.
//! 5. Budget (all kinds): `daily_llm_call_count >= stage_llm_budget`
//!    refuses the reservation.
//! 6. On Ok, increment `daily_llm_call_count` + the per-kind counter
//!    + record `last_llm_dispatch_at`. The caller is then expected to
//!    persist via `save_state` before spawning any async task so a
//!    crash/restart cannot let the same Vivling spend past its cap.
//! 7. On Err, increment the matching skip counter and bubble up the
//!    reason. `daily_llm_call_count` is NOT incremented.
//!
//! The mutator is deliberately synchronous and side-effect free
//! beyond touching `&mut VivlingState`: it does not call the network,
//! does not touch the file system, and never panics on user input.
//! Persistence + async dispatch are caller responsibilities (Step
//! 12.B.C / 12.B.D).

use chrono::DateTime;
use chrono::Utc;
use codex_vivling_core::model::LLM_EXPRESSION_THROTTLE_SECONDS;
use codex_vivling_core::model::LlmCallSkipReason;
use codex_vivling_core::model::Stage;
use codex_vivling_core::model::VivlingExpressionMode;
use codex_vivling_core::model::VivlingLlmCallKind;
use codex_vivling_core::model::stage_llm_budget;

use super::VivlingState;

impl VivlingState {
    /// Reset the daily LLM counters if `now` lies on a UTC day later
    /// than the one recorded in `daily_llm_day_key`. No-op when the
    /// day matches (or when the key is empty — first reservation of
    /// the Vivling's life).
    ///
    /// Public to `state_llm` only via the `pub(crate)` visibility on
    /// the wrapping `try_reserve_llm_call`; exposed here for tests
    /// that exercise the reset path independently.
    pub(crate) fn maybe_reset_daily_counters(&mut self, now: DateTime<Utc>) {
        let today = now.format("%Y-%m-%d").to_string();
        if self.daily_llm_day_key == today {
            return;
        }
        self.daily_llm_call_count = 0;
        self.daily_llm_chat_calls = 0;
        self.daily_llm_assist_calls = 0;
        self.daily_llm_loop_tick_calls = 0;
        self.daily_llm_expression_calls = 0;
        self.daily_llm_failure_count = 0;
        self.daily_llm_throttle_skips = 0;
        self.daily_llm_dedup_skips = 0;
        self.daily_llm_budget_skips = 0;
        self.daily_llm_optout_skips = 0;
        self.daily_llm_day_key = today;
        // `last_llm_dispatch_at` deliberately survives the daily
        // reset: the 60s throttle is wall-clock, not day-scoped.
    }

    /// Try to atomically reserve one LLM call slot.
    ///
    /// `prompt_hash` is consulted only when `kind == Expression` and
    /// the relevant cached entry (`cached_crt_phrase` for now; Step
    /// 12.B.D extends this) is still within its TTL. Other kinds
    /// ignore it.
    ///
    /// On success, mutates the daily counters + `last_llm_dispatch_at`
    /// and returns `Ok(())`. The caller MUST persist state before
    /// spawning the dispatch task.
    pub(crate) fn try_reserve_llm_call(
        &mut self,
        kind: VivlingLlmCallKind,
        now: DateTime<Utc>,
        prompt_hash: Option<u64>,
    ) -> Result<(), LlmCallSkipReason> {
        self.maybe_reset_daily_counters(now);

        // (1) Stage eligibility. Memory V2 Step 12.B.E removes the
        // Baby+Chat refusal: every stage may dispatch `/vl` through
        // the LLM, with `stage_guidance_section` modulating the tone
        // (Baby = tiny voice, observing, simple words). The previous
        // `NotEligibleStage` arm is preserved on the enum so future
        // kinds (e.g. an Adult-only autonomous channel) can reuse it.
        let _ = Stage::Baby; // anchor: legacy reference for the comment above.

        // (2) Expression mode opt-out. Chat / Assist / LoopTick are
        // not gated by `crt_brain_mode`: that flag governs only the
        // always-on Expression channel.
        if matches!(kind, VivlingLlmCallKind::Expression)
            && self.crt_brain_mode == VivlingExpressionMode::Off
        {
            self.daily_llm_optout_skips = self.daily_llm_optout_skips.saturating_add(1);
            return Err(LlmCallSkipReason::OptOut);
        }

        // (3) Throttle. Only applies to Expression so user-initiated
        // chat/assist/loop calls answer immediately. The daily budget
        // alone caps the user-initiated paths.
        if matches!(kind, VivlingLlmCallKind::Expression)
            && let Some(prev) = self.last_llm_dispatch_at
            && now.signed_duration_since(prev).num_seconds() < LLM_EXPRESSION_THROTTLE_SECONDS
        {
            self.daily_llm_throttle_skips = self.daily_llm_throttle_skips.saturating_add(1);
            return Err(LlmCallSkipReason::Throttle);
        }

        // (4) Dedup. Only meaningful for Expression: the planner
        // produces a deterministic prompt from state, and if it
        // matches the still-fresh cache there is no new signal to
        // spend the LLM budget on.
        if matches!(kind, VivlingLlmCallKind::Expression)
            && let Some(new_hash) = prompt_hash
        {
            let cached_crt_matches = self
                .cached_crt_phrase
                .as_ref()
                .map(|c| {
                    let fresh = c.ttl_expires_at.map(|exp| exp > now).unwrap_or(false);
                    fresh && c.prompt_hash == Some(new_hash)
                })
                .unwrap_or(false);
            if cached_crt_matches {
                self.daily_llm_dedup_skips = self.daily_llm_dedup_skips.saturating_add(1);
                return Err(LlmCallSkipReason::Dedup);
            }
        }

        // (5) Budget. Stage-scoped cap, shared across all kinds.
        let cap = stage_llm_budget(self.stage());
        if self.daily_llm_call_count >= cap {
            self.daily_llm_budget_skips = self.daily_llm_budget_skips.saturating_add(1);
            return Err(LlmCallSkipReason::BudgetExhausted);
        }

        // OK — atomically increment the counters. Callers must
        // `save_state` before spawning the dispatch task.
        self.daily_llm_call_count = self.daily_llm_call_count.saturating_add(1);
        match kind {
            VivlingLlmCallKind::Chat => {
                self.daily_llm_chat_calls = self.daily_llm_chat_calls.saturating_add(1);
            }
            VivlingLlmCallKind::Assist => {
                self.daily_llm_assist_calls = self.daily_llm_assist_calls.saturating_add(1);
            }
            VivlingLlmCallKind::LoopTick => {
                self.daily_llm_loop_tick_calls = self.daily_llm_loop_tick_calls.saturating_add(1);
            }
            VivlingLlmCallKind::Expression => {
                self.daily_llm_expression_calls = self.daily_llm_expression_calls.saturating_add(1);
            }
        }
        self.last_llm_dispatch_at = Some(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vivling::model::SeedIdentity;
    use codex_vivling_core::model::ADULT_LEVEL;
    use codex_vivling_core::model::CachedCrtPhrase;
    use codex_vivling_core::model::JUVENILE_LEVEL;

    fn seed() -> VivlingState {
        VivlingState::new(SeedIdentity {
            value: "step-12bb-fixture".to_string(),
            install_id: None,
        })
    }

    fn adult() -> VivlingState {
        let mut s = seed();
        s.level = ADULT_LEVEL;
        assert_eq!(s.stage(), Stage::Adult);
        s
    }

    fn juvenile() -> VivlingState {
        let mut s = seed();
        s.level = JUVENILE_LEVEL;
        assert_eq!(s.stage(), Stage::Juvenile);
        s
    }

    fn baby() -> VivlingState {
        let s = seed();
        assert_eq!(s.stage(), Stage::Baby);
        s
    }

    fn t(date: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(date)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn reset_day_key_on_new_utc_day() {
        let mut s = adult();
        s.daily_llm_day_key = "2026-05-20".to_string();
        s.daily_llm_call_count = 50;
        s.daily_llm_chat_calls = 10;
        s.daily_llm_throttle_skips = 3;

        s.maybe_reset_daily_counters(t("2026-05-21T00:00:30Z"));

        assert_eq!(s.daily_llm_day_key, "2026-05-21");
        assert_eq!(s.daily_llm_call_count, 0);
        assert_eq!(s.daily_llm_chat_calls, 0);
        assert_eq!(s.daily_llm_throttle_skips, 0);
    }

    #[test]
    fn reset_is_noop_when_same_day() {
        let mut s = adult();
        s.daily_llm_day_key = "2026-05-21".to_string();
        s.daily_llm_call_count = 50;
        s.maybe_reset_daily_counters(t("2026-05-21T23:59:00Z"));
        assert_eq!(s.daily_llm_call_count, 50, "same-day reset must be a no-op");
    }

    #[test]
    fn reserve_ok_increments_call_count_and_kind_counter() {
        let mut s = adult();
        let now = t("2026-05-21T10:00:00Z");
        s.try_reserve_llm_call(VivlingLlmCallKind::Chat, now, None)
            .expect("Adult Chat reservation should succeed");
        assert_eq!(s.daily_llm_call_count, 1);
        assert_eq!(s.daily_llm_chat_calls, 1);
        assert_eq!(s.daily_llm_expression_calls, 0);
        assert_eq!(s.last_llm_dispatch_at, Some(now));
        assert_eq!(s.daily_llm_day_key, "2026-05-21");
    }

    #[test]
    fn reserve_baby_chat_succeeds_after_step_12_b_e_unlock() {
        // Step 12.B.E (post-alpha smoke test): Baby+Chat is no longer
        // a NotEligibleStage refusal. Baby answers `/vl` through the
        // LLM with the `stage_guidance_section` tone modulation. The
        // reservation primitive must therefore accept Baby+Chat as
        // a normal billable call.
        let mut s = baby();
        let now = t("2026-05-21T10:00:00Z");
        s.try_reserve_llm_call(VivlingLlmCallKind::Chat, now, None)
            .expect("Baby+Chat must be allowed post-12.B.E");
        assert_eq!(s.daily_llm_chat_calls, 1);
        assert_eq!(s.daily_llm_call_count, 1);
    }

    #[test]
    fn reserve_baby_expression_passes_eligibility_stage_check() {
        // Baby is structurally allowed for Expression (rare-event channel).
        // We don't actually want Baby chat over LLM but expression refresh
        // is fine.
        let mut s = baby();
        let now = t("2026-05-21T10:00:00Z");
        s.try_reserve_llm_call(VivlingLlmCallKind::Expression, now, None)
            .expect("Baby Expression eligible");
        assert_eq!(s.daily_llm_expression_calls, 1);
    }

    #[test]
    fn reserve_expression_with_opt_out_off_increments_optout_skip() {
        let mut s = adult();
        s.crt_brain_mode = VivlingExpressionMode::Off;
        let err = s
            .try_reserve_llm_call(
                VivlingLlmCallKind::Expression,
                t("2026-05-21T10:00:00Z"),
                None,
            )
            .unwrap_err();
        assert_eq!(err, LlmCallSkipReason::OptOut);
        assert_eq!(s.daily_llm_optout_skips, 1);
        assert_eq!(s.daily_llm_call_count, 0);
    }

    #[test]
    fn reserve_chat_ignores_opt_out_off() {
        // crt_brain_mode governs only Expression. Chat must still
        // pass even with Off so /vl Juvenile/Adult keeps working.
        let mut s = juvenile();
        s.crt_brain_mode = VivlingExpressionMode::Off;
        s.try_reserve_llm_call(VivlingLlmCallKind::Chat, t("2026-05-21T10:00:00Z"), None)
            .expect("Chat reservation must not be blocked by crt-brain off");
        assert_eq!(s.daily_llm_chat_calls, 1);
    }

    #[test]
    fn reserve_expression_throttle_within_60s_skips() {
        let mut s = adult();
        let first = t("2026-05-21T10:00:00Z");
        s.try_reserve_llm_call(VivlingLlmCallKind::Expression, first, None)
            .expect("first Expression OK");
        // 30s later — within 60s window.
        let second = t("2026-05-21T10:00:30Z");
        let err = s
            .try_reserve_llm_call(VivlingLlmCallKind::Expression, second, None)
            .unwrap_err();
        assert_eq!(err, LlmCallSkipReason::Throttle);
        assert_eq!(s.daily_llm_throttle_skips, 1);
        assert_eq!(
            s.daily_llm_expression_calls, 1,
            "throttle must not bill the second attempt"
        );
    }

    #[test]
    fn reserve_chat_ignores_throttle_window() {
        // Throttle is Expression-only; Chat answers immediately.
        let mut s = adult();
        let first = t("2026-05-21T10:00:00Z");
        s.try_reserve_llm_call(VivlingLlmCallKind::Chat, first, None)
            .expect("first Chat OK");
        let second = t("2026-05-21T10:00:01Z");
        s.try_reserve_llm_call(VivlingLlmCallKind::Chat, second, None)
            .expect("second Chat within 1s must still succeed");
        assert_eq!(s.daily_llm_chat_calls, 2);
        assert_eq!(s.daily_llm_throttle_skips, 0);
    }

    #[test]
    fn reserve_expression_dedup_when_prompt_hash_matches_fresh_cache() {
        let mut s = adult();
        // Seed the CRT cache with a fresh entry pointing at hash 42.
        let now = t("2026-05-21T10:00:00Z");
        s.cached_crt_phrase = Some(CachedCrtPhrase {
            text: "cached output".to_string(),
            generated_at: Some(now),
            prompt_hash: Some(42),
            ttl_expires_at: Some(t("2026-05-21T10:10:00Z")),
        });
        // Step past the 60s throttle window so the dedup branch is reached.
        let later = t("2026-05-21T10:02:00Z");
        let err = s
            .try_reserve_llm_call(VivlingLlmCallKind::Expression, later, Some(42))
            .unwrap_err();
        assert_eq!(err, LlmCallSkipReason::Dedup);
        assert_eq!(s.daily_llm_dedup_skips, 1);
        assert_eq!(s.daily_llm_call_count, 0);
    }

    #[test]
    fn reserve_expression_dedup_ignored_when_cache_is_stale() {
        let mut s = adult();
        let now = t("2026-05-21T10:00:00Z");
        s.cached_crt_phrase = Some(CachedCrtPhrase {
            text: "stale".to_string(),
            generated_at: Some(t("2026-05-21T09:00:00Z")),
            prompt_hash: Some(42),
            ttl_expires_at: Some(t("2026-05-21T09:10:00Z")),
        });
        s.try_reserve_llm_call(VivlingLlmCallKind::Expression, now, Some(42))
            .expect("stale cache must not block dedup");
        assert_eq!(s.daily_llm_expression_calls, 1);
        assert_eq!(s.daily_llm_dedup_skips, 0);
    }

    #[test]
    fn reserve_budget_exhausted_at_exactly_cap() {
        let mut s = adult();
        s.daily_llm_call_count = stage_llm_budget(Stage::Adult);
        s.daily_llm_day_key = "2026-05-21".to_string();
        let err = s
            .try_reserve_llm_call(VivlingLlmCallKind::Chat, t("2026-05-21T10:00:00Z"), None)
            .unwrap_err();
        assert_eq!(err, LlmCallSkipReason::BudgetExhausted);
        assert_eq!(s.daily_llm_budget_skips, 1);
        assert_eq!(
            s.daily_llm_call_count,
            stage_llm_budget(Stage::Adult),
            "budget rejection must not bill"
        );
    }

    #[test]
    fn reserve_resets_counters_on_new_day_before_billing() {
        let mut s = adult();
        s.daily_llm_day_key = "2026-05-20".to_string();
        s.daily_llm_call_count = stage_llm_budget(Stage::Adult);
        s.try_reserve_llm_call(VivlingLlmCallKind::Chat, t("2026-05-21T08:00:00Z"), None)
            .expect("new day must reset budget");
        assert_eq!(s.daily_llm_call_count, 1);
        assert_eq!(s.daily_llm_day_key, "2026-05-21");
    }
}
