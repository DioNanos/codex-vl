//! Memory V2 Step 12.B.D.2 — async expression dispatch path.
//!
//! Pure-logic module bridging the deterministic expression planner
//! ([`codex_vivling_core::model::plan_expression_prompt`]) with the
//! async LLM runner in [`crate::app::vivling_background`]. Trigger
//! sites are intentionally NOT wired here: Step 12.B.D.3 cables the
//! `/vivling crt-brain` command and the post-turn refresh hook.
//!
//! Contract:
//! 1. [`maybe_dispatch_expression_refresh`] consumes the planned
//!    prompt + its hash, atomically reserves an Expression slot via
//!    [`VivlingState::try_reserve_llm_call`], and returns the
//!    [`VivlingExpressionRequest`] for the caller to dispatch. The
//!    caller MUST `save_state` BEFORE spawning the async task — the
//!    daily LLM counter increments are persisted state and a crash
//!    between reservation and dispatch would otherwise allow the same
//!    Vivling to spend past its cap.
//! 2. [`record_expression_result`] consumes the validated LLM reply
//!    and writes both cache slots ([`CachedCrtPhrase`] and
//!    [`CachedProactive`]) with stage-aware TTL. The fields are
//!    `#[serde(skip)]` runtime-only (Step 12.B.D.1) so no `save_state`
//!    is needed after this call.
//! 3. [`parse_expression_reply`] tolerantly extracts a `{crt_phrase,
//!    proactive}` JSON object from the raw LLM output: it strips
//!    markdown code fences, scans for the first balanced JSON object,
//!    and only then hands it to `serde`. Bounded validation +
//!    `redact_secrets` defense are applied inside
//!    [`record_expression_result`] so a malformed/leaky reply cannot
//!    poison the cache.

use chrono::DateTime;
use chrono::Duration;
use chrono::Utc;
use codex_vivling_core::model::CachedCrtPhrase;
use codex_vivling_core::model::CachedProactive;
use codex_vivling_core::model::LLM_EXPRESSION_THROTTLE_SECONDS;
use codex_vivling_core::model::LOOP_EXPRESSION_HEADROOM_DENOMINATOR;
use codex_vivling_core::model::LOOP_EXPRESSION_THROTTLE_SECONDS;
use codex_vivling_core::model::Stage;
use codex_vivling_core::model::VivlingExpressionMode;
use codex_vivling_core::model::VivlingLlmCallKind;
use codex_vivling_core::model::fnv1a64;
use codex_vivling_core::model::plan_expression_prompt;
use codex_vivling_core::model::stage_llm_budget;
use codex_vivling_core::model::truncate_summary;
use codex_vivling_core::redaction::redact_secrets;
use serde::Deserialize;

use super::BrainTarget;
use super::request::resolve_expression_target;
use crate::vivling::model::VivlingState;

/// Maximum chars for the CRT footer phrase. Matches the renderer's
/// visual budget (single-line footer slot).
const EXPRESSION_CRT_MAX: usize = 28;
/// Maximum chars for the proactive message (longer than CRT — used
/// in the chat surface, not the footer).
const EXPRESSION_PROACTIVE_MAX: usize = 120;
/// Stage-aware TTL: Adult/Juvenile refresh more often.
const TTL_ADULT_JUVENILE_MINUTES: i64 = 10;
/// Baby Expression is a rare-event channel — a fresh phrase is
/// cheaper to keep around longer.
const TTL_BABY_MINUTES: i64 = 30;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VivlingExpressionRequest {
    pub(crate) vivling_id: String,
    pub(crate) vivling_name: String,
    pub(crate) brain_target: BrainTarget,
    pub(crate) prompt: String,
    pub(crate) language: String,
    pub(crate) prompt_hash: u64,
    pub(crate) generated_at: DateTime<Utc>,
    /// Memory V2 Step 12.B.J — focus hint derived from the live
    /// context (current task, active loop label, agent label, bond
    /// tone). When `Some(_)`, the async runner appends it to the
    /// Expression system instruction so the CRT footer can reflect
    /// what the Vivling is observing RIGHT NOW (e.g. `"merge upstream
    /// watch"`, `"vps3 bootstrap focus"`) instead of generic platitudes
    /// (`"loops breathe, work hums"`). Hashed into `prompt_hash` so a
    /// focus shift triggers a fresh dispatch, not a dedup skip.
    pub(crate) focus_hint: Option<String>,
    /// Memory V2 Step 12.B.L — bootstrap dispatch flag. `true` only
    /// for the one-shot dispatch issued by `ensure_startup_dispatched`
    /// at TUI session start. The async runner uses this to enrich the
    /// system instruction with a "first phrase of the session" hint
    /// so the LLM greets in the resolved language instead of producing
    /// a generic CRT phrase. Default `false` for all other dispatch
    /// paths (turn-driven, loop-driven, idle frame, forced refresh).
    pub(crate) bootstrap: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VivlingExpressionResult {
    pub(crate) vivling_id: String,
    pub(crate) crt_phrase: Option<String>,
    pub(crate) proactive: Option<String>,
    pub(crate) prompt_hash: u64,
    pub(crate) generated_at: DateTime<Utc>,
}

/// Wire format the LLM is asked to emit. Both fields are optional so
/// a partial reply (only CRT, only proactive) still parses.
#[derive(Debug, Deserialize)]
struct ExpressionReplyPayload {
    #[serde(default)]
    crt_phrase: Option<String>,
    #[serde(default)]
    proactive: Option<String>,
}

/// Atomically reserve one Expression slot for `state` and build the
/// dispatch request. Returns `None` when the reservation is refused
/// (throttle / dedup / budget / opt-out / etc.) so the renderer keeps
/// falling back to the cached/template chain.
///
/// On `Some(_)`, the daily LLM counters + `last_llm_dispatch_at` are
/// mutated. The caller MUST persist `state` via `save_state` BEFORE
/// spawning the background dispatch task; otherwise a crash could
/// allow the slot to be re-billed on restart.
///
/// Call sites land in Step 12.B.D.3 (`/vivling crt-brain` command +
/// post-turn refresh hook); the helper is wired now so the path can
/// be reviewed and tested in isolation.
#[allow(dead_code)]
pub(crate) fn maybe_dispatch_expression_refresh(
    state: &mut VivlingState,
    now: DateTime<Utc>,
    prompt: String,
    language: String,
    prompt_hash: u64,
    focus_hint: Option<String>,
    bootstrap: bool,
) -> Option<VivlingExpressionRequest> {
    state
        .try_reserve_llm_call(VivlingLlmCallKind::Expression, now, Some(prompt_hash))
        .ok()?;
    let brain_target =
        resolve_expression_target(state.brain_enabled, state.brain_profile.as_deref());
    Some(VivlingExpressionRequest {
        vivling_id: state.vivling_id.clone(),
        vivling_name: state.name.clone(),
        brain_target,
        prompt,
        language,
        prompt_hash,
        generated_at: now,
        focus_hint,
        bootstrap,
    })
}

/// Apply the validated Expression LLM reply on the main thread.
/// Writes both runtime cache slots with stage-aware TTL.
///
/// Independence: if only one of the two fields is present (or only
/// one survives sanitization), the other cache slot is left intact.
/// Sanitization runs `redact_secrets` then `truncate_summary` so a
/// leaky / over-long reply cannot poison the cache.
pub(crate) fn record_expression_result(
    state: &mut VivlingState,
    reply: &VivlingExpressionResult,
    now: DateTime<Utc>,
) {
    let stage = state.stage();
    let ttl_minutes = if matches!(stage, Stage::Baby) {
        TTL_BABY_MINUTES
    } else {
        TTL_ADULT_JUVENILE_MINUTES
    };
    let expires = now + Duration::minutes(ttl_minutes);

    if let Some(text) = sanitize_phrase(reply.crt_phrase.as_deref(), EXPRESSION_CRT_MAX) {
        state.cached_crt_phrase = Some(CachedCrtPhrase {
            text,
            generated_at: Some(now),
            prompt_hash: Some(reply.prompt_hash),
            ttl_expires_at: Some(expires),
        });
    }
    if let Some(text) = sanitize_phrase(reply.proactive.as_deref(), EXPRESSION_PROACTIVE_MAX) {
        state.cached_proactive = Some(CachedProactive {
            text,
            generated_at: Some(now),
            prompt_hash: Some(reply.prompt_hash),
            ttl_expires_at: Some(expires),
        });
    }
}

/// Record an Expression failure on `state.daily_llm_failure_count`.
/// The counter is persisted (V10 schema field), so the caller is
/// expected to `save_state` after this mutation.
pub(crate) fn record_expression_failure(state: &mut VivlingState) {
    state.daily_llm_failure_count = state.daily_llm_failure_count.saturating_add(1);
}

/// Memory V2 Step 12.B.D.3 — best-effort end-to-end planner +
/// reservation. Serialize `state` to its on-disk JSON projection,
/// hand it to the deterministic [`plan_expression_prompt`] in core,
/// hash the planned prompt with FNV-1a, and try to reserve an
/// Expression slot. Returns `None` whenever any step refuses
/// (serialization failure, planner skipped, throttle / dedup /
/// budget / opt-out, …) so post-turn callers can stay tolerant —
/// every refusal is a normal outcome.
///
/// Caller MUST `save_state` BEFORE spawning the dispatch task: the
/// daily LLM counters mutated by the reservation are persisted state
/// and a crash between reservation and dispatch would otherwise
/// allow the slot to be re-billed.
/// Memory V2 Step 12.B.J — build a short focus hint string from the
/// live context + Vivling bond tone. Returns `None` when nothing
/// concrete is available (live_context empty, no active task).
/// Bounded to ~160 chars so the focus line cannot crowd the
/// Expression system instruction.
pub(crate) fn build_focus_hint(
    state: &VivlingState,
    live: Option<&super::live_context::VivlingLiveContext>,
) -> Option<String> {
    let live = live?;
    let mut parts: Vec<String> = Vec::new();
    if let Some(task) = live
        .task_progress
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parts.push(format!("task `{}`", truncate_summary(task, 80)));
    }
    if let Some(agent) = live
        .active_agent_label
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parts.push(format!("agent `{}`", truncate_summary(agent, 40)));
    }
    if let Some(thread) = live
        .thread_title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parts.push(format!("thread `{}`", truncate_summary(thread, 40)));
    }
    // Bond tone modulates voice register but does not gate inclusion.
    let tone_label = match state.bond.tone() {
        crate::vivling::BondTone::Neutral => "neutral",
        crate::vivling::BondTone::Warm => "warm",
        crate::vivling::BondTone::Familiar => "familiar",
    };
    if parts.is_empty() {
        return None;
    }
    let joined = parts.join(", ");
    let bounded = truncate_summary(&joined, 140);
    Some(format!("{bounded} · tone {tone_label}"))
}

pub(crate) fn try_plan_and_reserve_expression(
    state: &mut VivlingState,
    now: DateTime<Utc>,
    focus_hint: Option<String>,
) -> Option<VivlingExpressionRequest> {
    // Cheap pre-flight: skip the planner entirely when the user has
    // muted the channel. Saves a serialization + planner pass per
    // turn for Vivlings the user opted out of.
    if state.crt_brain_mode == VivlingExpressionMode::Off {
        return None;
    }
    // Memory V2 Step 12.B.H: high-frequency callers (the per-frame
    // `pre_draw_tick` idle hook) hit this helper many times per
    // second. Skip the JSON serialization + planner pass when the
    // 60s Expression throttle window is still open — `try_reserve`
    // would refuse anyway, but only after the expensive work. This
    // is purely an optimization; correctness is unchanged.
    if let Some(prev) = state.last_llm_dispatch_at
        && now.signed_duration_since(prev).num_seconds() < LLM_EXPRESSION_THROTTLE_SECONDS
    {
        return None;
    }
    // Memory V2 Step 12.B.I (DAG smoke test 2026-05-22 evidenza
    // `dedup_skips=2979` in poche ore): when the CRT cache is still
    // fresh, every per-frame idle call would run the planner and
    // then bump `daily_llm_dedup_skips` through `try_reserve`. The
    // counter is supposed to surface user-facing observability,
    // not "this Vivling has been idle for 10 minutes with a still-
    // valid cache". Skip here before incurring serde + planner
    // when both: (a) cache is fresh (TTL in the future), and
    // (b) cache was produced by an LLM dispatch (prompt_hash set).
    // Forced refresh (`try_plan_and_reserve_expression_forced`)
    // bypasses this short-circuit by clearing `last_llm_dispatch_at`
    // upstream; the dedup gate inside `try_reserve` still runs and
    // protects content-unchanged refusals on the forced path.
    if let Some(cached) = state.cached_crt_phrase.as_ref()
        && cached.prompt_hash.is_some()
        && cached.ttl_expires_at.map(|exp| exp > now).unwrap_or(false)
    {
        return None;
    }
    let body = serde_json::to_string(state).ok()?;
    let plan = plan_expression_prompt(&body, now).ok()?.ok()?;
    // Memory V2 Step 12.B.J — fold the focus hint into the prompt
    // hash so a focus shift (e.g. user switches task) triggers a
    // fresh dispatch instead of a dedup skip against the previous
    // generic phrase.
    let mut hash_bytes = plan.prompt.as_bytes().to_vec();
    if let Some(focus) = focus_hint.as_deref() {
        hash_bytes.push(b'\0');
        hash_bytes.extend_from_slice(focus.as_bytes());
    }
    let prompt_hash = fnv1a64(&hash_bytes);
    maybe_dispatch_expression_refresh(
        state,
        now,
        plan.prompt,
        plan.language,
        prompt_hash,
        focus_hint,
        false,
    )
}

/// Memory V2 Step 12.B.H — force-refresh variant of
/// [`try_plan_and_reserve_expression`] used by the
/// `/vivling crt-brain refresh` command. Bypasses the 60s
/// Expression throttle by temporarily clearing
/// `last_llm_dispatch_at` before the standard pipeline runs;
/// budget / opt-out / dedup gates all still apply. The cleared
/// timestamp is restored by `maybe_dispatch_expression_refresh`
/// when it bills the slot on success, so the regular throttle
/// resumes from this dispatch onwards.
pub(crate) fn try_plan_and_reserve_expression_forced(
    state: &mut VivlingState,
    now: DateTime<Utc>,
    focus_hint: Option<String>,
) -> Option<VivlingExpressionRequest> {
    if state.crt_brain_mode == VivlingExpressionMode::Off {
        return None;
    }
    // Save / clear / restore-on-failure pattern (Sonnet P1 fix
    // 2026-05-21). The forced path clears `last_llm_dispatch_at`
    // so the inner throttle gate stops refusing, but if the
    // pipeline still refuses for any other reason (dedup, budget,
    // planner no-source, opt-out via mode toggle race, …) we
    // MUST restore the anchor. Otherwise the pre-flight throttle
    // check inside `try_plan_and_reserve_expression` no longer
    // short-circuits the per-frame `pre_draw_tick` idle hook —
    // serde + planner would spin every frame until the next
    // genuine dispatch resets the anchor.
    let saved_dispatch_at = state.last_llm_dispatch_at;
    state.last_llm_dispatch_at = None;
    let result = try_plan_forced_inner(state, now, focus_hint);
    if result.is_none() {
        state.last_llm_dispatch_at = saved_dispatch_at;
    }
    result
}

fn try_plan_forced_inner(
    state: &mut VivlingState,
    now: DateTime<Utc>,
    focus_hint: Option<String>,
) -> Option<VivlingExpressionRequest> {
    let body = serde_json::to_string(state).ok()?;
    let plan = plan_expression_prompt(&body, now).ok()?.ok()?;
    let mut hash_bytes = plan.prompt.as_bytes().to_vec();
    if let Some(focus) = focus_hint.as_deref() {
        hash_bytes.push(b'\0');
        hash_bytes.extend_from_slice(focus.as_bytes());
    }
    let prompt_hash = fnv1a64(&hash_bytes);
    maybe_dispatch_expression_refresh(
        state,
        now,
        plan.prompt,
        plan.language,
        prompt_hash,
        focus_hint,
        false,
    )
}

/// Memory V2 Step 12.B.L — bootstrap dispatch issued once per
/// session by `ensure_startup_dispatched`. Differs from the regular
/// pipeline in two ways:
///
/// 1. **Bypasses the 60s `last_llm_dispatch_at` throttle**: the boot
///    moment is exactly when the user expects a greeting; a stale
///    timestamp from the previous session must not silence it.
///    Re-uses the save/clear/restore pattern from the forced refresh
///    so any other refusal (dedup against fresh cache, opt-out,
///    budget exhausted) leaves `last_llm_dispatch_at` intact for the
///    rest of the session.
/// 2. **Sets `request.bootstrap = true`** so the async runner can
///    enrich the system instruction with a "first phrase of the
///    session, greet in {language}" hint. Without that hint the LLM
///    routinely defaults to English for non-EN Vivlings, defeating
///    the whole point of starting localized.
///
/// Returns `None` on every refusal so the caller keeps showing the
/// cached/template chain. Pre-flight cache-fresh skip is preserved:
/// if the previous session left a fresh `cached_crt_phrase` with a
/// valid `prompt_hash`, the dispatch is skipped (it is preferable
/// to show the last real phrase than to spend a slot on a redundant
/// LLM call at every restart).
pub(crate) fn try_plan_and_reserve_expression_bootstrap(
    state: &mut VivlingState,
    now: DateTime<Utc>,
    focus_hint: Option<String>,
) -> Option<VivlingExpressionRequest> {
    if state.crt_brain_mode == VivlingExpressionMode::Off {
        return None;
    }
    // Pre-flight cache-fresh skip (Step 12.B.I parity): on a normal
    // restart we want to keep showing the previous phrase, not burn a
    // slot to regenerate it. Only when the cache is missing / expired
    // / template-derived do we proceed.
    if let Some(cached) = state.cached_crt_phrase.as_ref()
        && cached.prompt_hash.is_some()
        && cached.ttl_expires_at.map(|exp| exp > now).unwrap_or(false)
    {
        return None;
    }
    let saved_dispatch_at = state.last_llm_dispatch_at;
    state.last_llm_dispatch_at = None;
    let result = try_plan_bootstrap_inner(state, now, focus_hint);
    if result.is_none() {
        state.last_llm_dispatch_at = saved_dispatch_at;
    }
    result
}

fn try_plan_bootstrap_inner(
    state: &mut VivlingState,
    now: DateTime<Utc>,
    focus_hint: Option<String>,
) -> Option<VivlingExpressionRequest> {
    let body = serde_json::to_string(state).ok()?;
    let plan = plan_expression_prompt(&body, now).ok()?.ok()?;
    let mut hash_bytes = plan.prompt.as_bytes().to_vec();
    if let Some(focus) = focus_hint.as_deref() {
        hash_bytes.push(b'\0');
        hash_bytes.extend_from_slice(focus.as_bytes());
    }
    // Step 12.B.L: fold a stable "boot" marker into the hash so the
    // bootstrap dispatch never collides with the regular dedup gate
    // even if the planned prompt happens to match a previous one.
    hash_bytes.push(b'\0');
    hash_bytes.extend_from_slice(b"boot");
    let prompt_hash = fnv1a64(&hash_bytes);
    maybe_dispatch_expression_refresh(
        state,
        now,
        plan.prompt,
        plan.language,
        prompt_hash,
        focus_hint,
        true,
    )
}

/// Memory V2 Step 12.B.D.4 — anti-burn variant of
/// [`try_plan_and_reserve_expression`] for loop-event hooks. Loop
/// ticks fire much more frequently than turn completions; this
/// helper layers three extra refusal gates on top of the standard
/// reservation pipeline:
///
/// 1. **Stage gate** — only `Adult` Vivlings receive loop-driven
///    refreshes. Baby + Juvenile loop events are too noisy and
///    short-on-context to spend an LLM call on.
/// 2. **Dedicated 5-minute throttle** — `last_loop_expression_dispatch_at`
///    is independent from `last_llm_dispatch_at`, so turn-driven
///    refreshes keep their standard 60s window while loop hooks pay
///    a stricter floor.
/// 3. **Budget headroom 50%** — when ≥ 50% of today's stage budget
///    is already consumed, the loop hook stops triggering so the
///    remaining headroom is reserved for turn-driven refreshes
///    (editorial choice: turn snapshots carry fresher signal).
///
/// On `Some(_)`, `last_loop_expression_dispatch_at` is set to `now`
/// BEFORE returning so the caller's `save_state` persists both the
/// `try_reserve` mutation and the loop throttle bookkeeping in one
/// atomic write.
pub(crate) fn try_plan_and_reserve_expression_for_loop(
    state: &mut VivlingState,
    now: DateTime<Utc>,
    focus_hint: Option<String>,
) -> Option<VivlingExpressionRequest> {
    if !matches!(state.stage(), Stage::Adult) {
        return None;
    }
    if let Some(prev) = state.last_loop_expression_dispatch_at
        && now.signed_duration_since(prev).num_seconds() < LOOP_EXPRESSION_THROTTLE_SECONDS
    {
        return None;
    }
    let cap = stage_llm_budget(state.stage());
    if state
        .daily_llm_call_count
        .saturating_mul(LOOP_EXPRESSION_HEADROOM_DENOMINATOR)
        > cap
    {
        return None;
    }
    let request = try_plan_and_reserve_expression(state, now, focus_hint)?;
    state.last_loop_expression_dispatch_at = Some(now);
    Some(request)
}

/// Memory V2 Step 12.B.D.3 — human-readable summary for
/// `/vivling crt-brain` (and the inline status block of
/// `/vivling crt-brain show`). Always returns something printable
/// even for a brand-new Vivling with zero counters.
pub(crate) fn format_crt_brain_status(state: &VivlingState) -> String {
    let mode = match state.crt_brain_mode {
        VivlingExpressionMode::Default => "default (stage-driven)",
        VivlingExpressionMode::On => "on (forced)",
        VivlingExpressionMode::Off => "off (muted)",
    };
    let day_key = if state.daily_llm_day_key.is_empty() {
        "(no calls yet)"
    } else {
        state.daily_llm_day_key.as_str()
    };
    // Step 12.B.O — render the effective cap (override-aware) so the
    // user sees what `try_reserve_llm_call` will actually enforce.
    // `Unlimited` is rendered as `∞` instead of `u32::MAX` to keep the
    // status line readable.
    let cap = state.budget_override.effective_cap(state.stage());
    let cap_label = match state.budget_override {
        codex_vivling_core::model::VivlingBudgetCap::Unlimited => "∞".to_string(),
        _ => cap.to_string(),
    };
    let remaining = cap.saturating_sub(state.daily_llm_call_count);
    let remaining_label = match state.budget_override {
        codex_vivling_core::model::VivlingBudgetCap::Unlimited => "∞".to_string(),
        _ => remaining.to_string(),
    };
    let mut lines = Vec::with_capacity(7);
    lines.push(format!("CRT brain: {mode}"));
    lines.push(format!(
        "Budget: {}",
        state.budget_override.label(state.stage())
    ));
    lines.push(format!("Day: {day_key}"));
    lines.push(format!(
        "Calls today: total {}/{} ({} left) (chat {}, assist {}, loop {}, expression {})",
        state.daily_llm_call_count,
        cap_label,
        remaining_label,
        state.daily_llm_chat_calls,
        state.daily_llm_assist_calls,
        state.daily_llm_loop_tick_calls,
        state.daily_llm_expression_calls,
    ));
    lines.push(format!(
        "Skips today: throttle {}, dedup {}, budget {}, opt-out {}",
        state.daily_llm_throttle_skips,
        state.daily_llm_dedup_skips,
        state.daily_llm_budget_skips,
        state.daily_llm_optout_skips,
    ));
    lines.push(format!(
        "Failures today: failures {}",
        state.daily_llm_failure_count
    ));
    lines.join("\n")
}

fn sanitize_phrase(raw: Option<&str>, max_chars: usize) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        return None;
    }
    let redacted = redact_secrets(trimmed);
    let bounded = truncate_summary(redacted.trim(), max_chars);
    let final_trimmed = bounded.trim();
    if final_trimmed.is_empty() {
        None
    } else {
        Some(final_trimmed.to_string())
    }
}

/// Tolerant parse of the Expression LLM reply.
///
/// Many providers stubbornly wrap JSON in markdown fences or
/// prepend an apology paragraph; this helper strips fences and
/// extracts the first balanced JSON object before handing it to
/// `serde`. Returns `(crt_phrase, proactive)` as raw `Option<String>`
/// — sanitization (redaction + bounding) is `record_expression_result`'s
/// job so the parser stays a pure transform.
pub(crate) fn parse_expression_reply(
    raw: &str,
) -> Result<(Option<String>, Option<String>), String> {
    let stripped = strip_markdown_fence(raw);
    let candidate = first_json_object(stripped.trim())
        .ok_or_else(|| "Vivling expression reply did not contain a JSON object.".to_string())?;
    let payload: ExpressionReplyPayload = serde_json::from_str(candidate)
        .map_err(|err| format!("Vivling expression reply was not valid JSON: {err}"))?;
    Ok((payload.crt_phrase, payload.proactive))
}

fn strip_markdown_fence(raw: &str) -> &str {
    let mut s = raw.trim();
    for prefix in ["```json", "```JSON", "```"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.trim_start();
            break;
        }
    }
    if let Some(rest) = s.strip_suffix("```") {
        s = rest.trim_end();
    }
    s
}

fn first_json_object(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (idx, &b) in bytes.iter().enumerate().skip(start) {
        if escape {
            escape = false;
            continue;
        }
        if in_string {
            match b {
                b'\\' => escape = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..=idx]);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vivling::model::SeedIdentity;
    use codex_vivling_core::model::ADULT_LEVEL;
    use codex_vivling_core::model::JUVENILE_LEVEL;
    use codex_vivling_core::model::VivlingExpressionMode;
    use codex_vivling_core::model::stage_llm_budget;

    fn seed() -> VivlingState {
        VivlingState::new(SeedIdentity {
            value: "step-12bd2-fixture".to_string(),
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

    fn make_reply(
        crt: Option<&str>,
        proactive: Option<&str>,
        hash: u64,
    ) -> VivlingExpressionResult {
        VivlingExpressionResult {
            vivling_id: "vid".to_string(),
            crt_phrase: crt.map(str::to_string),
            proactive: proactive.map(str::to_string),
            prompt_hash: hash,
            generated_at: t("2026-05-21T10:00:00Z"),
        }
    }

    // ---- parser ----------------------------------------------------

    #[test]
    fn parse_accepts_bare_json_object() {
        let (crt, proactive) =
            parse_expression_reply(r#"{"crt_phrase":"hello","proactive":"world"}"#).unwrap();
        assert_eq!(crt.as_deref(), Some("hello"));
        assert_eq!(proactive.as_deref(), Some("world"));
    }

    #[test]
    fn parse_strips_markdown_fence_json_tag() {
        let raw = "```json\n{\"crt_phrase\":\"hi\"}\n```";
        let (crt, proactive) = parse_expression_reply(raw).unwrap();
        assert_eq!(crt.as_deref(), Some("hi"));
        assert!(proactive.is_none());
    }

    #[test]
    fn parse_strips_plain_fence_and_extracts_first_object() {
        let raw = "```\nSorry, here is the JSON:\n{\"proactive\":\"only this\"}\n```";
        let (crt, proactive) = parse_expression_reply(raw).unwrap();
        assert!(crt.is_none());
        assert_eq!(proactive.as_deref(), Some("only this"));
    }

    #[test]
    fn parse_extracts_first_object_when_preceded_by_prose() {
        let raw = "Apologies for the delay. {\"crt_phrase\":\"ok\"} thanks!";
        let (crt, _proactive) = parse_expression_reply(raw).unwrap();
        assert_eq!(crt.as_deref(), Some("ok"));
    }

    #[test]
    fn parse_balances_braces_inside_strings() {
        // The first balanced object closes after the nested string's
        // literal "}" — the scanner must ignore braces inside quotes.
        let raw = r#"{"crt_phrase":"text with }} braces","proactive":"ok"}"#;
        let (crt, proactive) = parse_expression_reply(raw).unwrap();
        assert_eq!(crt.as_deref(), Some("text with }} braces"));
        assert_eq!(proactive.as_deref(), Some("ok"));
    }

    #[test]
    fn parse_errors_when_no_json_object_present() {
        let err = parse_expression_reply("no braces at all").unwrap_err();
        assert!(
            err.contains("did not contain a JSON object"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_errors_when_object_is_malformed() {
        let err = parse_expression_reply(r#"{"crt_phrase":}"#).unwrap_err();
        assert!(err.contains("not valid JSON"), "unexpected error: {err}");
    }

    // ---- record_expression_result ---------------------------------

    #[test]
    fn record_writes_both_caches_with_adult_ttl() {
        let mut s = adult();
        let now = t("2026-05-21T10:00:00Z");
        let reply = make_reply(Some("hi adult"), Some("plan a task"), 7);
        record_expression_result(&mut s, &reply, now);

        let crt = s.cached_crt_phrase.as_ref().expect("crt cached");
        assert_eq!(crt.text, "hi adult");
        assert_eq!(crt.prompt_hash, Some(7));
        assert_eq!(crt.generated_at, Some(now));
        assert_eq!(
            crt.ttl_expires_at,
            Some(now + Duration::minutes(TTL_ADULT_JUVENILE_MINUTES))
        );

        let proactive = s.cached_proactive.as_ref().expect("proactive cached");
        assert_eq!(proactive.text, "plan a task");
        assert_eq!(proactive.prompt_hash, Some(7));
        assert_eq!(
            proactive.ttl_expires_at,
            Some(now + Duration::minutes(TTL_ADULT_JUVENILE_MINUTES))
        );
    }

    #[test]
    fn record_uses_longer_baby_ttl() {
        let mut s = baby();
        let now = t("2026-05-21T10:00:00Z");
        let reply = make_reply(Some("watch"), None, 1);
        record_expression_result(&mut s, &reply, now);
        let crt = s.cached_crt_phrase.as_ref().expect("crt cached");
        assert_eq!(
            crt.ttl_expires_at,
            Some(now + Duration::minutes(TTL_BABY_MINUTES))
        );
    }

    #[test]
    fn record_leaves_existing_proactive_when_reply_only_has_crt() {
        let mut s = adult();
        let now = t("2026-05-21T10:00:00Z");
        s.cached_proactive = Some(CachedProactive {
            text: "kept".to_string(),
            generated_at: Some(now),
            prompt_hash: Some(99),
            ttl_expires_at: Some(now + Duration::minutes(5)),
        });
        let reply = make_reply(Some("fresh crt"), None, 1);
        record_expression_result(&mut s, &reply, now);
        assert_eq!(s.cached_crt_phrase.as_ref().unwrap().text, "fresh crt");
        assert_eq!(
            s.cached_proactive.as_ref().unwrap().text,
            "kept",
            "missing proactive field must not wipe the existing cache slot"
        );
    }

    #[test]
    fn record_skips_empty_and_whitespace_phrases() {
        let mut s = adult();
        let now = t("2026-05-21T10:00:00Z");
        let reply = make_reply(Some("   "), Some(""), 1);
        record_expression_result(&mut s, &reply, now);
        assert!(s.cached_crt_phrase.is_none());
        assert!(s.cached_proactive.is_none());
    }

    #[test]
    fn record_redacts_secrets_before_caching() {
        let mut s = adult();
        let now = t("2026-05-21T10:00:00Z");
        // A real-shaped GitHub token must be redacted; sanitize_phrase
        // also length-bounds, so the test focuses on the redaction
        // signal rather than exact replacement string.
        let reply = make_reply(Some("ghp_1234567890abcdefghijklmnopqrstuvwxyzAB"), None, 1);
        record_expression_result(&mut s, &reply, now);
        let crt = s.cached_crt_phrase.as_ref().expect("crt cached");
        assert!(
            !crt.text.contains("ghp_1234567890abcdef"),
            "raw secret leaked into CRT cache: {}",
            crt.text
        );
    }

    #[test]
    fn record_truncates_overlong_proactive() {
        let mut s = adult();
        let now = t("2026-05-21T10:00:00Z");
        let long = "a".repeat(EXPRESSION_PROACTIVE_MAX * 3);
        let reply = make_reply(None, Some(&long), 1);
        record_expression_result(&mut s, &reply, now);
        let proactive = s.cached_proactive.as_ref().expect("proactive cached");
        // `truncate_summary` keeps `max_chars` chars and appends the
        // 3-char "..." ellipsis when it has to cut — the cache budget
        // therefore peaks at `max + 3`, never higher.
        let count = proactive.text.chars().count();
        assert!(
            count <= EXPRESSION_PROACTIVE_MAX + 3,
            "proactive cache must respect the budget (max + ellipsis), got {count} chars"
        );
        assert!(
            count > EXPRESSION_PROACTIVE_MAX,
            "this test feeds an oversize string; the truncation suffix should fire, got {count} chars"
        );
        assert!(
            proactive.text.ends_with("..."),
            "truncated proactive must end with the truncation suffix"
        );
    }

    // ---- maybe_dispatch_expression_refresh ------------------------

    #[test]
    fn dispatch_ok_returns_request_and_bills_counter() {
        let mut s = adult();
        let now = t("2026-05-21T10:00:00Z");
        let req = maybe_dispatch_expression_refresh(
            &mut s,
            now,
            "hello prompt".to_string(),
            "en".to_string(),
            42,
            None,
            false,
        )
        .expect("Adult Expression dispatch should reserve");
        assert_eq!(req.prompt, "hello prompt");
        assert_eq!(req.language, "en");
        assert_eq!(req.prompt_hash, 42);
        assert_eq!(req.brain_target, BrainTarget::SessionDefault);
        assert_eq!(s.daily_llm_call_count, 1);
        assert_eq!(s.daily_llm_expression_calls, 1);
        assert_eq!(s.last_llm_dispatch_at, Some(now));
    }

    #[test]
    fn dispatch_returns_none_when_mode_off() {
        let mut s = adult();
        s.crt_brain_mode = VivlingExpressionMode::Off;
        let now = t("2026-05-21T10:00:00Z");
        let req = maybe_dispatch_expression_refresh(
            &mut s,
            now,
            "p".to_string(),
            "en".to_string(),
            1,
            None,
            false,
        );
        assert!(req.is_none(), "Off mode must refuse dispatch");
        assert_eq!(s.daily_llm_optout_skips, 1);
        assert_eq!(s.daily_llm_call_count, 0);
    }

    #[test]
    fn dispatch_returns_none_when_budget_exhausted() {
        let mut s = adult();
        s.daily_llm_call_count = stage_llm_budget(Stage::Adult);
        s.daily_llm_day_key = "2026-05-21".to_string();
        let now = t("2026-05-21T10:00:00Z");
        let req = maybe_dispatch_expression_refresh(
            &mut s,
            now,
            "p".to_string(),
            "en".to_string(),
            1,
            None,
            false,
        );
        assert!(req.is_none(), "budget cap must refuse dispatch");
        assert_eq!(s.daily_llm_budget_skips, 1);
    }

    #[test]
    fn dispatch_returns_none_when_dedup_matches_fresh_cache() {
        let mut s = adult();
        let now = t("2026-05-21T10:00:00Z");
        s.cached_crt_phrase = Some(CachedCrtPhrase {
            text: "cached".to_string(),
            generated_at: Some(now),
            prompt_hash: Some(7),
            ttl_expires_at: Some(t("2026-05-21T10:30:00Z")),
        });
        // Past the 60s throttle so the dedup branch is reached.
        let later = t("2026-05-21T10:02:00Z");
        let req = maybe_dispatch_expression_refresh(
            &mut s,
            later,
            "p".to_string(),
            "en".to_string(),
            7,
            None,
            false,
        );
        assert!(req.is_none(), "matching fresh cache must dedup");
        assert_eq!(s.daily_llm_dedup_skips, 1);
    }

    #[test]
    fn dispatch_baby_returns_request_rare_event_channel() {
        // Baby Expression is intentionally allowed: it is the rare-event
        // channel (`Default` mode triggers only on hatch / turn_complete
        // upstream, but the reservation primitive must not refuse).
        let mut s = baby();
        let now = t("2026-05-21T10:00:00Z");
        let req = maybe_dispatch_expression_refresh(
            &mut s,
            now,
            "p".to_string(),
            "en".to_string(),
            1,
            None,
            false,
        );
        assert!(
            req.is_some(),
            "Baby Expression eligible — caller decides rarity upstream"
        );
        assert_eq!(s.daily_llm_expression_calls, 1);
    }

    // ---- P1 recovery from Step 12.B.D.2 audit (Sonnet 4.6 Max) ----

    #[test]
    fn dispatch_returns_none_when_throttle_window_active() {
        // P1.1 recovery: prove the 60s Expression throttle short-circuits
        // the dispatch path even when the cache is empty (no dedup signal
        // available). Seeds `last_llm_dispatch_at` 30s in the past so the
        // try_reserve call hits the throttle branch.
        let mut s = adult();
        s.daily_llm_day_key = "2026-05-21".to_string();
        s.last_llm_dispatch_at = Some(t("2026-05-21T09:59:30Z"));
        let now = t("2026-05-21T10:00:00Z");
        let req = maybe_dispatch_expression_refresh(
            &mut s,
            now,
            "p".to_string(),
            "en".to_string(),
            1,
            None,
            false,
        );
        assert!(req.is_none(), "throttle window must refuse dispatch");
        assert_eq!(s.daily_llm_throttle_skips, 1);
        assert_eq!(
            s.daily_llm_call_count, 0,
            "throttle-rejected reservation must not bill"
        );
    }

    #[test]
    fn record_leaves_existing_crt_when_reply_only_has_proactive() {
        // P1.2 recovery: symmetric counterpart of
        // record_leaves_existing_proactive_when_reply_only_has_crt — a
        // proactive-only reply must not wipe the CRT cache slot.
        let mut s = adult();
        let now = t("2026-05-21T10:00:00Z");
        s.cached_crt_phrase = Some(CachedCrtPhrase {
            text: "kept crt".to_string(),
            generated_at: Some(now),
            prompt_hash: Some(99),
            ttl_expires_at: Some(now + Duration::minutes(5)),
        });
        let reply = make_reply(None, Some("fresh proactive"), 1);
        record_expression_result(&mut s, &reply, now);
        assert_eq!(
            s.cached_crt_phrase.as_ref().unwrap().text,
            "kept crt",
            "missing crt field must not wipe the existing cache slot"
        );
        assert_eq!(s.cached_proactive.as_ref().unwrap().text, "fresh proactive");
    }

    #[test]
    fn record_truncates_overlong_crt_phrase() {
        // P1.3 recovery: symmetric counterpart of
        // record_truncates_overlong_proactive for the CRT slot. The CRT
        // budget is small (28 chars), so an oversize string is the most
        // realistic poisoning vector.
        let mut s = adult();
        let now = t("2026-05-21T10:00:00Z");
        let long = "x".repeat(EXPRESSION_CRT_MAX * 3);
        let reply = make_reply(Some(&long), None, 1);
        record_expression_result(&mut s, &reply, now);
        let crt = s.cached_crt_phrase.as_ref().expect("crt cached");
        let count = crt.text.chars().count();
        assert!(
            count <= EXPRESSION_CRT_MAX + 3,
            "crt cache must respect the budget (max + ellipsis), got {count} chars"
        );
        assert!(
            count > EXPRESSION_CRT_MAX,
            "this test feeds an oversize string; truncation suffix should fire, got {count} chars"
        );
        assert!(
            crt.text.ends_with("..."),
            "truncated crt must end with the truncation suffix"
        );
    }

    // ---- 12.B.D.4 try_plan_and_reserve_expression_for_loop --------

    fn adult_with_voice() -> VivlingState {
        // Make the planner produce a prompt: set self_voice non-empty
        // so the source-material gate inside plan_expression_prompt
        // is satisfied.
        let mut s = adult();
        s.hatched = true;
        s.self_voice = Some(codex_vivling_core::model::VivlingVoice {
            text: "I keep watch over the build pipeline.".to_string(),
            language: "en".to_string(),
            generated_at: Some(t("2026-05-21T09:00:00Z")),
            source_capsules_count: 5,
            version: 1,
        });
        s
    }

    #[test]
    fn loop_dispatch_returns_none_for_baby_stage() {
        let mut s = baby();
        s.hatched = true;
        s.self_voice = Some(codex_vivling_core::model::VivlingVoice {
            text: "tiny voice".to_string(),
            language: "en".to_string(),
            generated_at: None,
            source_capsules_count: 1,
            version: 1,
        });
        assert!(
            try_plan_and_reserve_expression_for_loop(&mut s, t("2026-05-21T10:00:00Z"), None)
                .is_none()
        );
        assert_eq!(s.daily_llm_expression_calls, 0, "stage gate must not bill");
    }

    #[test]
    fn loop_dispatch_returns_none_for_juvenile_stage() {
        let mut s = juvenile();
        s.hatched = true;
        s.self_voice = Some(codex_vivling_core::model::VivlingVoice {
            text: "growing voice".to_string(),
            language: "en".to_string(),
            generated_at: None,
            source_capsules_count: 1,
            version: 1,
        });
        assert!(
            try_plan_and_reserve_expression_for_loop(&mut s, t("2026-05-21T10:00:00Z"), None)
                .is_none()
        );
    }

    #[test]
    fn loop_dispatch_returns_some_for_adult_first_call() {
        let mut s = adult_with_voice();
        let now = t("2026-05-21T10:00:00Z");
        let req = try_plan_and_reserve_expression_for_loop(&mut s, now, None);
        assert!(req.is_some(), "Adult first loop dispatch should succeed");
        assert_eq!(s.last_loop_expression_dispatch_at, Some(now));
        assert_eq!(s.daily_llm_expression_calls, 1);
    }

    #[test]
    fn loop_dispatch_returns_none_within_5min_throttle() {
        let mut s = adult_with_voice();
        s.last_loop_expression_dispatch_at = Some(t("2026-05-21T09:58:00Z"));
        s.daily_llm_day_key = "2026-05-21".to_string();
        // 2 minutes later — well inside the 5-minute floor.
        let now = t("2026-05-21T10:00:00Z");
        assert!(try_plan_and_reserve_expression_for_loop(&mut s, now, None).is_none());
        assert_eq!(s.daily_llm_expression_calls, 0);
    }

    #[test]
    fn loop_dispatch_allows_after_5min_throttle_expires() {
        let mut s = adult_with_voice();
        s.last_loop_expression_dispatch_at = Some(t("2026-05-21T09:54:00Z"));
        let now = t("2026-05-21T10:00:00Z"); // 6 minutes later
        assert!(try_plan_and_reserve_expression_for_loop(&mut s, now, None).is_some());
    }

    #[test]
    fn loop_dispatch_returns_none_when_budget_over_50_percent() {
        let mut s = adult_with_voice();
        let cap = stage_llm_budget(Stage::Adult);
        // Seed counter at exactly 50% + 1 so the headroom gate fires.
        s.daily_llm_call_count = cap / 2 + 1;
        s.daily_llm_day_key = "2026-05-21".to_string();
        let now = t("2026-05-21T10:00:00Z");
        assert!(try_plan_and_reserve_expression_for_loop(&mut s, now, None).is_none());
        assert_eq!(
            s.daily_llm_expression_calls, 0,
            "headroom gate must not bill"
        );
    }

    #[test]
    fn loop_dispatch_allows_at_exactly_50_percent_budget() {
        let mut s = adult_with_voice();
        let cap = stage_llm_budget(Stage::Adult);
        // Exactly at 50% — the gate is `> cap` (after `*2`), so equal
        // must still pass.
        s.daily_llm_call_count = cap / 2;
        s.daily_llm_day_key = "2026-05-21".to_string();
        let now = t("2026-05-21T10:00:00Z");
        assert!(try_plan_and_reserve_expression_for_loop(&mut s, now, None).is_some());
    }

    #[test]
    fn loop_dispatch_independent_from_turn_throttle_window() {
        // last_llm_dispatch_at (turn-driven) within 60s should NOT
        // block the loop helper directly — the loop helper has its
        // own dedicated 5min throttle. The shared 60s window inside
        // try_reserve will still fire downstream, however, because
        // both paths funnel through the same reservation primitive.
        let mut s = adult_with_voice();
        s.last_llm_dispatch_at = Some(t("2026-05-21T09:59:30Z"));
        let now = t("2026-05-21T10:00:00Z"); // 30s later
        let req = try_plan_and_reserve_expression_for_loop(&mut s, now, None);
        assert!(
            req.is_none(),
            "shared 60s throttle inside try_reserve should still refuse — \
             loop-specific gates pass, but inner reservation does not"
        );
        // The refusal must come from the inner throttle, not the loop
        // helper's own bookkeeping — last_loop_expression_dispatch_at
        // must stay None.
        assert_eq!(s.last_loop_expression_dispatch_at, None);
    }

    // ---- 12.B.D.3 try_plan_and_reserve_expression ------------------

    #[test]
    fn try_plan_and_reserve_returns_none_when_mode_off_short_circuit() {
        // Pre-flight optimization: muted mode must skip serialization
        // and planner entirely, returning None without billing or
        // touching the opt-out skip counter (the dispatcher arm runs
        // only when try_reserve fires).
        let mut s = adult();
        s.crt_brain_mode = VivlingExpressionMode::Off;
        let req = try_plan_and_reserve_expression(&mut s, t("2026-05-21T10:00:00Z"), None);
        assert!(req.is_none());
        assert_eq!(
            s.daily_llm_optout_skips, 0,
            "pre-flight skip must not bill the optout counter — only try_reserve does"
        );
    }

    #[test]
    fn pre_flight_skips_when_cache_fresh_to_prevent_dedup_explosion() {
        // Memory V2 Step 12.B.I (DAG smoke test 2026-05-22):
        // pre_draw_tick idle hook calls this helper at frame rate.
        // When the cache is still fresh, the inner dedup gate would
        // bump `daily_llm_dedup_skips` once per frame — DAG observed
        // 2979 in a few hours. The pre-flight check must short-
        // circuit BEFORE serde + planner + try_reserve.
        let mut s = adult_with_voice();
        let now = t("2026-05-21T10:00:00Z");
        s.last_llm_dispatch_at = Some(t("2026-05-21T09:55:00Z"));
        s.cached_crt_phrase = Some(CachedCrtPhrase {
            text: "cached".to_string(),
            generated_at: Some(now),
            prompt_hash: Some(42),
            ttl_expires_at: Some(now + Duration::minutes(10)),
        });
        let dedup_before = s.daily_llm_dedup_skips;
        // Past the 60s throttle so the throttle pre-flight would
        // not catch this — the cache-fresh pre-flight must.
        let later = t("2026-05-21T10:02:00Z");
        let req = try_plan_and_reserve_expression(&mut s, later, None);
        assert!(req.is_none(), "fresh cache pre-flight must skip");
        assert_eq!(
            s.daily_llm_dedup_skips, dedup_before,
            "pre-flight must NOT bump dedup_skips counter (only forced/explicit \
             paths bill it)"
        );
        assert_eq!(s.daily_llm_call_count, 0);
        assert_eq!(s.daily_llm_expression_calls, 0);
    }

    #[test]
    fn pre_flight_skips_when_throttle_window_open_to_prevent_dedup_explosion() {
        // Cache fresh AND throttle window open — both should
        // short-circuit. Test confirms the simpler throttle gate
        // still wins when cache is empty (no prompt_hash).
        let mut s = adult_with_voice();
        let now = t("2026-05-21T10:00:00Z");
        s.last_llm_dispatch_at = Some(t("2026-05-21T09:59:30Z")); // 30s ago
        let dedup_before = s.daily_llm_dedup_skips;
        let req = try_plan_and_reserve_expression(&mut s, now, None);
        assert!(req.is_none(), "throttle pre-flight must skip");
        assert_eq!(s.daily_llm_dedup_skips, dedup_before);
        assert_eq!(
            s.daily_llm_throttle_skips, 0,
            "pre-flight must not bill throttle skip"
        );
    }

    #[test]
    fn forced_dispatch_restores_throttle_anchor_on_dedup_failure() {
        // Memory V2 Step 12.B.H P1 (Sonnet 2026-05-21): the forced
        // refresh path clears `last_llm_dispatch_at` to bypass the
        // 60s throttle. If the pipeline still refuses (here: dedup
        // against a still-fresh cached_crt_phrase that matches the
        // planner prompt hash), the anchor MUST be restored, or
        // `try_plan_and_reserve_expression`'s pre-flight throttle
        // would no longer short-circuit per-frame idle calls —
        // serde + planner would spin until the next real dispatch.
        let mut s = adult_with_voice();
        let saved = Some(t("2026-05-21T09:55:00Z"));
        s.last_llm_dispatch_at = saved;
        let now = t("2026-05-21T10:00:00Z");

        // Prime the dedup gate: serialize-plan-hash the state once,
        // then stuff a still-fresh cache entry with the same hash.
        let body = serde_json::to_string(&s).expect("serialize state");
        let plan = plan_expression_prompt(&body, now)
            .expect("planner ok")
            .expect("planner has source material");
        let prompt_hash = fnv1a64(plan.prompt.as_bytes());
        s.cached_crt_phrase = Some(CachedCrtPhrase {
            text: "cached".to_string(),
            generated_at: Some(now),
            prompt_hash: Some(prompt_hash),
            ttl_expires_at: Some(now + Duration::minutes(10)),
        });

        let result = try_plan_and_reserve_expression_forced(&mut s, now, None);
        assert!(result.is_none(), "dedup must refuse the forced dispatch");
        assert_eq!(
            s.last_llm_dispatch_at, saved,
            "throttle anchor must be restored on failure so the pre-flight \
             check in try_plan_and_reserve_expression keeps short-circuiting"
        );
    }

    #[test]
    fn try_plan_and_reserve_returns_none_when_planner_has_no_source_material() {
        // Fresh hatched Adult with empty self_voice / distilled / work
        // memory: the planner refuses with `NoSourceMaterial`, so the
        // helper must surface None without billing.
        let mut s = adult();
        s.hatched = true;
        let req = try_plan_and_reserve_expression(&mut s, t("2026-05-21T10:00:00Z"), None);
        assert!(
            req.is_none(),
            "planner refusal must propagate as None without billing"
        );
        assert_eq!(s.daily_llm_call_count, 0);
    }

    // ---- format_crt_brain_status -----------------------------------

    #[test]
    fn format_crt_brain_status_pretty_prints_mode_labels_distinctly() {
        let mut s = adult();
        let default_text = format_crt_brain_status(&s);
        s.crt_brain_mode = VivlingExpressionMode::On;
        let on_text = format_crt_brain_status(&s);
        s.crt_brain_mode = VivlingExpressionMode::Off;
        let off_text = format_crt_brain_status(&s);
        assert_ne!(default_text, on_text);
        assert_ne!(default_text, off_text);
        assert_ne!(on_text, off_text);
    }

    #[test]
    fn dispatch_uses_profile_target_when_brain_on_with_profile() {
        let mut s = juvenile();
        s.brain_enabled = true;
        s.brain_profile = Some("glm".to_string());
        let now = t("2026-05-21T10:00:00Z");
        let req = maybe_dispatch_expression_refresh(
            &mut s,
            now,
            "p".to_string(),
            "en".to_string(),
            1,
            None,
            false,
        )
        .expect("Juvenile Expression dispatch should reserve");
        assert_eq!(req.brain_target, BrainTarget::Profile("glm".to_string()));
    }

    // ---- 12.B.J build_focus_hint + hash fold (Sonnet P1) ----------

    #[test]
    fn build_focus_hint_returns_none_when_live_missing() {
        let s = adult();
        assert!(build_focus_hint(&s, None).is_none());
    }

    #[test]
    fn build_focus_hint_returns_none_when_all_fields_empty() {
        let s = adult();
        let live = super::super::live_context::VivlingLiveContext::default();
        assert!(build_focus_hint(&s, Some(&live)).is_none());
    }

    #[test]
    fn build_focus_hint_returns_none_when_fields_are_whitespace_only() {
        let s = adult();
        let live = super::super::live_context::VivlingLiveContext {
            task_progress: Some("   ".to_string()),
            active_agent_label: Some("\t".to_string()),
            thread_title: Some("".to_string()),
            ..Default::default()
        };
        assert!(
            build_focus_hint(&s, Some(&live)).is_none(),
            "whitespace-only fields must be treated as empty"
        );
    }

    #[test]
    fn build_focus_hint_task_only_when_agent_thread_absent() {
        let s = adult();
        let live = super::super::live_context::VivlingLiveContext {
            task_progress: Some("merge upstream".to_string()),
            active_agent_label: None,
            thread_title: None,
            ..Default::default()
        };
        let hint = build_focus_hint(&s, Some(&live)).expect("task set → Some");
        assert!(hint.contains("task `merge upstream`"), "{hint}");
        assert!(!hint.contains("agent"), "agent must be absent: {hint}");
        assert!(!hint.contains("thread"), "thread must be absent: {hint}");
        assert!(hint.contains("· tone "), "{hint}");
    }

    #[test]
    fn build_focus_hint_all_three_fields_and_tone_present() {
        let s = adult();
        let live = super::super::live_context::VivlingLiveContext {
            task_progress: Some("step 12".to_string()),
            active_agent_label: Some("Nilo".to_string()),
            thread_title: Some("codex-vl audit".to_string()),
            ..Default::default()
        };
        let hint = build_focus_hint(&s, Some(&live)).expect("all fields → Some");
        assert!(hint.contains("task `step 12`"), "{hint}");
        assert!(hint.contains("agent `Nilo`"), "{hint}");
        assert!(hint.contains("thread `codex-vl audit`"), "{hint}");
        assert!(hint.contains("· tone "), "{hint}");
    }

    #[test]
    fn focus_hint_changes_produce_different_prompt_hash() {
        // Hash fold invariant (Step 12.B.J): identical prompt, different
        // focus → different prompt_hash. Guarantees a focus shift breaks
        // the dedup gate and triggers a fresh dispatch on the next TTL-
        // expired pass.
        let prompt = b"test prompt";
        let focus_a = "merge upstream";
        let focus_b = "vps3 bootstrap";

        let mut bytes_a = prompt.to_vec();
        bytes_a.push(b'\0');
        bytes_a.extend_from_slice(focus_a.as_bytes());

        let mut bytes_b = prompt.to_vec();
        bytes_b.push(b'\0');
        bytes_b.extend_from_slice(focus_b.as_bytes());

        assert_ne!(
            fnv1a64(&bytes_a),
            fnv1a64(&bytes_b),
            "different focus must produce different hash"
        );

        // Same focus must be byte-equal → same hash (determinism).
        let mut bytes_a2 = prompt.to_vec();
        bytes_a2.push(b'\0');
        bytes_a2.extend_from_slice(focus_a.as_bytes());
        assert_eq!(
            fnv1a64(&bytes_a),
            fnv1a64(&bytes_a2),
            "identical focus must produce identical hash"
        );
    }
}
