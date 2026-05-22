//! Structured prompt composition for the Vivling brain.
//!
//! The Vivling accumulates four very different kinds of signal:
//! identity bias, distilled patterns, recent observed work, and
//! the live status of the host TUI. Older code mixed them all into
//! a single memory digest, which made the brain treat stale loop
//! counters as proof of the current system state.
//!
//! `compose_brain_prompt` keeps each layer in its own section so the
//! brain can reason about *now* (live state + payload) without being
//! biased into a stale conclusion by historical counters.
//!
//! The `Live state contract:` and `Learned memory:` labels are kept
//! as substring anchors so existing TUI tests and downstream tooling
//! stay green; the layout gains explicit Identity / Stable memory /
//! Recent observed work / Live state (now) / Stale signals sections.
use super::super::model::Stage;
use super::super::model::VivlingState;
use super::super::model::truncate_summary;
use super::VivlingLiveContext;

#[derive(Debug, Clone)]
pub(crate) enum BrainPromptKind<'a> {
    Assist,
    Chat,
    LoopTick {
        label: &'a str,
        goal: &'a str,
        prompt_text: &'a str,
        auto_remove_on_completion: bool,
    },
}

impl<'a> BrainPromptKind<'a> {
    fn live_state_contract(&self) -> &'static str {
        match self {
            BrainPromptKind::Assist => {
                "Live state is unknown unless the task explicitly provides it. Treat learned memory as bias and history, not proof that the current system is blocked, idle, active, or complete."
            }
            BrainPromptKind::Chat => {
                "Live state is unknown unless the user message explicitly provides it. Treat learned memory as bias and history, not proof that the current system is blocked, idle, active, or complete."
            }
            BrainPromptKind::LoopTick { .. } => {
                "Live state is whatever the Live state (now) section reports plus the loop tick payload. Treat learned memory as bias and history, not proof that the current loop is blocked, idle, active, or complete."
            }
        }
    }

    fn payload_label(&self) -> &'static str {
        match self {
            BrainPromptKind::Assist => "Task",
            BrainPromptKind::Chat => "User message",
            BrainPromptKind::LoopTick { .. } => "Loop tick",
        }
    }
}

pub(crate) fn compose_brain_prompt(
    state: &VivlingState,
    kind: BrainPromptKind<'_>,
    payload: &str,
    live: Option<&VivlingLiveContext>,
    msa: Option<&super::msa::VivlingMsa>,
    skills: &[codex_vivling_core::model::VivlingSkill],
) -> Result<String, String> {
    if matches!(kind, BrainPromptKind::Assist) && state.stage() != Stage::Adult {
        return Err("`/vivling assist ...` unlocks only at level 60.".to_string());
    }
    if matches!(kind, BrainPromptKind::Assist) && !state.brain_enabled {
        return Err("Enable the Vivling brain first with `/vivling brain on`.".to_string());
    }
    // Memory V2 §8.1 (P0.2): the prompt's `profile` field is only a
    // display label inside `identity_section`. When the Vivling has no
    // explicit profile, surface the inheritance choice as
    // "session-default" instead of erroring out — the dispatcher resolves
    // the actual model via `BrainTarget::SessionDefault` downstream.
    let profile = state.brain_profile.as_deref().unwrap_or("session-default");
    let payload = payload.trim();
    if payload.is_empty() {
        return Err(match kind {
            BrainPromptKind::Assist => "Usage: /vivling assist <task>".to_string(),
            BrainPromptKind::Chat => "Usage: /vl <message>".to_string(),
            BrainPromptKind::LoopTick { .. } => "Loop tick payload is empty.".to_string(),
        });
    }

    let mut sections: Vec<String> = Vec::new();
    sections.push(identity_section(state, profile));
    if let Some(section) = self_voice_section(state) {
        sections.push(section);
    }
    if let Some(section) = lineage_inheritance_section(state) {
        sections.push(section);
    }
    if let Some(section) = skill_library_section(skills) {
        sections.push(section);
    }
    sections.push(stable_memory_section(state));
    sections.push(match msa {
        Some(msa) => retrieved_relevant_capsules_section(state, payload, msa),
        None => recent_observed_work_section(state),
    });
    sections.push(legacy_learned_memory_section(state));
    sections.push(live_state_section(live));
    if let Some(stale) = stale_signals_section(state) {
        sections.push(stale);
    }
    // codex-vl bond: relational steering hint, only on human-facing paths.
    // LoopTick is automation owned by the Vivling and must stay deterministic;
    // bond is a user-relationship signal, not an automation quality signal.
    if matches!(kind, BrainPromptKind::Chat | BrainPromptKind::Assist) {
        sections.push(format!("Bond:\n{}", state.bond.prompt_hint()));
    }
    sections.push(format!(
        "Live state contract:\n{}",
        kind.live_state_contract()
    ));
    sections.push(language_contract_section(state));
    sections.push(stage_guidance_section(state));
    // Memory V2 Step 12.B.P — opportunistic Ctrl+J mention. The user
    // can press Ctrl+J to open a dedicated chat panel; not every
    // user discovers this. The Vivling is told it exists and may
    // casually mention it if the conversation becomes long — never
    // forced, never every turn.
    sections.push(
        "TUI affordance: The user can press Ctrl+J to open a dedicated chat panel \
         where every exchange between you and them is preserved across turns. \
         If the conversation grows long or the user seems to lose track, you may \
         casually mention this option — but only when it would genuinely help, \
         never as a forced reminder."
            .to_string(),
    );
    if let BrainPromptKind::LoopTick {
        label,
        goal,
        prompt_text,
        auto_remove_on_completion,
    } = &kind
    {
        sections.push(format!(
            "Loop:\n- label: {label}\n- goal: {goal}\n- prompt: {prompt_text}\n- auto_remove_on_completion: {auto_remove_on_completion}\n- response contract: return strict JSON with fields status, message, optional loop_action; status is one of progress|blocked|done; loop_action.action may be none|disable|remove|trigger|update; for update you may optionally provide interval, goal, prompt, enabled.",
        ));
    }
    sections.push(format!("{}:\n{}", kind.payload_label(), payload));

    Ok(sections.join("\n\n"))
}

fn identity_section(state: &VivlingState, profile: &str) -> String {
    format!(
        "Vivling identity:\n- id: {}\n- name: {}\n- profile: {}\n- stage: {}\n- dominant role: {}\n- temperament: {}\n- brain potential: {}\n- tone: {}\n- verification bias: {}\n- caution bias: {}\n- question bias: {}",
        state.vivling_id,
        state.name,
        profile,
        state.stage().label(),
        state.dominant_archetype().label(),
        state.gene_vector.temperament_summary(),
        state.gene_vector.brain_potential_label(),
        state.identity_profile.tone,
        state.identity_profile.verification_bias,
        state.identity_profile.caution_bias,
        state.identity_profile.question_bias,
    )
}

fn stable_memory_section(state: &VivlingState) -> String {
    if state.distilled_summaries.is_empty() {
        return "Stable memory (distilled patterns):\n- (none yet — patterns still forming)"
            .to_string();
    }
    let mut lines = vec!["Stable memory (distilled patterns):".to_string()];
    for entry in state.distilled_summaries.iter().take(3) {
        lines.push(format!(
            "- {} [{}] x{}: {}",
            entry.topic,
            entry.archetype.label(),
            entry.observations,
            truncate_summary(&entry.summary, 96),
        ));
    }
    lines.join("\n")
}

fn recent_observed_work_section(state: &VivlingState) -> String {
    let observed: Vec<_> = state
        .work_memory
        .iter()
        .rev()
        .filter(|entry| entry.kind != "live_context")
        .take(5)
        .collect();
    if observed.is_empty() {
        return "Recent observed work:\n- (no observed work captured yet)".to_string();
    }
    let mut lines = vec!["Recent observed work:".to_string()];
    for entry in observed {
        lines.push(format!(
            "- {} [{}]: {}",
            entry.kind,
            entry.archetype.label(),
            truncate_summary(&entry.summary, 96),
        ));
    }
    lines.join("\n")
}

fn retrieved_relevant_capsules_section(
    state: &VivlingState,
    payload: &str,
    msa: &super::msa::VivlingMsa,
) -> String {
    let Some(idx) = msa.collection_for(&state.vivling_id) else {
        return recent_observed_work_section(state);
    };
    let hits = idx.search(payload, 5, None).unwrap_or_default();
    if hits.is_empty() {
        return recent_observed_work_section(state);
    }
    let mut lines = vec!["Relevant memory:".to_string()];
    for hit in hits {
        let kind = hit
            .metadata
            .get("kind")
            .and_then(|value| value.as_str())
            .unwrap_or("?");
        let archetype = hit
            .metadata
            .get("archetype")
            .and_then(|value| value.as_str())
            .unwrap_or("?");
        lines.push(format!(
            "- {} [{}] (rel {:.2}): {}",
            kind,
            archetype,
            hit.score,
            truncate_summary(&hit.snippet, 96),
        ));
    }
    lines.join("\n")
}

fn legacy_learned_memory_section(state: &VivlingState) -> String {
    let last = state
        .last_work_summary
        .as_deref()
        .map(|summary| truncate_summary(summary, 96))
        .unwrap_or_else(|| "No recent work summary yet.".to_string());
    format!(
        "Learned memory:\n- recent summary: {}\n- level {} · active_days {} · turns_observed {} · loop_exposure {}",
        last, state.level, state.active_work_days, state.turns_observed, state.loop_exposure,
    )
}

fn live_state_section(live: Option<&VivlingLiveContext>) -> String {
    match live.and_then(live_context_lines) {
        Some(lines) => format!("Live state (now):\n{lines}"),
        None => "Live state (now):\n- unknown — only the payload below reflects current state"
            .to_string(),
    }
}

fn live_context_lines(ctx: &VivlingLiveContext) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    if let Some(value) = clean(&ctx.run_state) {
        lines.push(format!("- run state: {value}"));
    }
    if let Some(value) = clean(&ctx.active_agent_label) {
        lines.push(format!("- active agent: {value}"));
    }
    if let Some(value) = clean(&ctx.task_progress) {
        lines.push(format!("- task progress: {value}"));
    }
    if let Some(value) = clean(&ctx.model) {
        lines.push(format!("- model: {value}"));
    }
    if let Some(value) = clean(&ctx.git_branch) {
        lines.push(format!("- git branch: {value}"));
    }
    if let Some(value) = clean(&ctx.cwd) {
        lines.push(format!("- cwd: {value}"));
    }
    if let Some(value) = clean(&ctx.thread_title) {
        lines.push(format!("- thread: {value}"));
    }
    if lines.is_empty() {
        return None;
    }
    Some(lines.join("\n"))
}

fn clean(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn stale_signals_section(state: &VivlingState) -> Option<String> {
    let blocked_total = state
        .loop_runtime_blocks
        .saturating_add(state.loop_admin_churn);
    if blocked_total == 0
        && state.loop_blocked_review == 0
        && state.loop_blocked_side == 0
        && state.loop_blocked_busy == 0
    {
        return None;
    }
    let mut lines = vec![
        "Stale signals (history, not proof of current state):".to_string(),
        format!(
            "- loop_runtime_blocks: {} (cumulative)",
            state.loop_runtime_blocks
        ),
        format!(
            "- loop_admin_churn: {} (cumulative)",
            state.loop_admin_churn
        ),
    ];
    if state.loop_blocked_review > 0 {
        lines.push(format!(
            "- loop_blocked_review: {} (cumulative)",
            state.loop_blocked_review
        ));
    }
    if state.loop_blocked_side > 0 {
        lines.push(format!(
            "- loop_blocked_side: {} (cumulative)",
            state.loop_blocked_side
        ));
    }
    if state.loop_blocked_busy > 0 {
        lines.push(format!(
            "- loop_blocked_busy: {} (cumulative)",
            state.loop_blocked_busy
        ));
    }
    Some(lines.join("\n"))
}

/// Memory V2 Step 5.A — language steering hint for the brain.
///
/// The Vivling answers in the language inherited from the user (axis G).
/// Mode controls the mix policy: `MirrorUser` rispecchia il mix utente,
/// `Strict` blocca sulla prima detection, `DominantOnly` insiste sulla
/// lingua dominante della finestra. La sezione viene aggiunta a tutti
/// i path (Assist, Chat, LoopTick) cosi' anche l'automation parla nella
/// lingua del proprietario.
/// Memory V2 Step 9.A — bounds applied to artifacts the memory agent
/// (Step 7.B / 8.B) writes into the prompt. The agent already runs
/// `redact_secrets`, but it does not cap field length: a future
/// LLM-enriched voice or a hand-edited sidecar could otherwise push
/// the prompt budget past the model's context. These limits are
/// deliberately conservative (a few sentences per field) so the
/// brain reasons on a tight identity sketch instead of a wall of
/// historical text.
const SELF_VOICE_TEXT_MAX: usize = 512;
const SKILL_NAME_MAX: usize = 80;
const SKILL_DESCRIPTION_MAX: usize = 200;
const SKILL_TRIGGER_MAX: usize = 32;
const SKILL_TRIGGERS_LIMIT: usize = 6;
const SKILL_STEP_MAX: usize = 96;
const SKILL_STEPS_LIMIT: usize = 4;
const SKILL_LIBRARY_LIMIT: usize = 5;
const LINEAGE_VOICE_FRAGMENT_MAX: usize = 240;
const LINEAGE_PROFILE_MAX: usize = 80;
const LINEAGE_INHERITED_SKILLS_LIMIT: usize = 3;

/// Memory V2 Step 9.A — surface the planner-written `self_voice` to
/// the brain prompt. Body is already redacted by the memory agent
/// (Step 7.B); we additionally cap the text length so a future
/// LLM-enriched voice cannot blow the prompt budget.
fn self_voice_section(state: &VivlingState) -> Option<String> {
    let voice = state.self_voice.as_ref()?;
    let text = voice.text.trim();
    if text.is_empty() {
        return None;
    }
    let language = if voice.language.trim().is_empty() {
        "(unset)".to_string()
    } else {
        voice.language.trim().to_string()
    };
    let bounded_text = truncate_summary(text, SELF_VOICE_TEXT_MAX);
    Some(format!(
        "Self voice ({language}, sources {count}):\n{bounded_text}",
        count = voice.source_capsules_count,
    ))
}

/// Memory V2 Step 9.A — bring in the skills sidecar produced by
/// Step 8.B. Caps at five entries, sorted by confidence (desc) then
/// by name (asc) for determinism. Each field is bounded so the
/// section's contribution to the prompt budget stays predictable
/// regardless of sidecar size. Skills whose name is empty after
/// trim are skipped — they cannot give the brain a usable handle.
fn skill_library_section(skills: &[codex_vivling_core::model::VivlingSkill]) -> Option<String> {
    if skills.is_empty() {
        return None;
    }
    let mut ordered: Vec<&codex_vivling_core::model::VivlingSkill> = skills
        .iter()
        .filter(|skill| !skill.name.trim().is_empty())
        .collect();
    if ordered.is_empty() {
        return None;
    }
    ordered.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
    let mut lines = vec!["Skill library:".to_string()];
    for skill in ordered.into_iter().take(SKILL_LIBRARY_LIMIT) {
        let name = truncate_summary(skill.name.trim(), SKILL_NAME_MAX);
        let bounded_triggers: Vec<String> = skill
            .trigger_keywords
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .take(SKILL_TRIGGERS_LIMIT)
            .map(|t| truncate_summary(t, SKILL_TRIGGER_MAX))
            .collect();
        let triggers = if bounded_triggers.is_empty() {
            "(none)".to_string()
        } else {
            bounded_triggers.join(", ")
        };
        let mut entry = format!(
            "- {name} [triggers: {triggers} | conf {conf:.2} | runs {ok}/{ko}]",
            conf = skill.confidence,
            ok = skill.success_count,
            ko = skill.failure_count,
        );
        let description = skill.description.trim();
        if !description.is_empty() {
            entry.push_str(&format!(
                "\n  desc: {}",
                truncate_summary(description, SKILL_DESCRIPTION_MAX)
            ));
        }
        let bounded_steps: Vec<String> = skill
            .step_sequence
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .take(SKILL_STEPS_LIMIT)
            .map(|s| truncate_summary(s, SKILL_STEP_MAX))
            .collect();
        if !bounded_steps.is_empty() {
            entry.push_str(&format!("\n  steps: {}", bounded_steps.join(" → ")));
        }
        lines.push(entry);
    }
    Some(lines.join("\n"))
}

/// Memory V2 Step 10.B — Axis D lineage inheritance.
///
/// Surfaces the parent's identity seed the child carries from
/// `create_spawned_offspring` (Step 10.A). The section is bounded,
/// optional, and never duplicates Step 9.A's `Self voice:` or
/// `Skill library:` content — those describe the *child's own* state,
/// this one describes what the child *inherited but has not yet
/// claimed as its own*.
///
/// Returns `None` when no signal would land in the prompt: no seed
/// at all, or a seed whose every field is empty/default. This keeps
/// child Vivlings without a meaningful lineage byte-identical to the
/// pre-Step-10.B prompt.
fn lineage_inheritance_section(state: &VivlingState) -> Option<String> {
    let seed = state.lineage_inheritance.as_ref()?;

    let voice_fragment = seed
        .voice_fragment
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| truncate_summary(s, LINEAGE_VOICE_FRAGMENT_MAX));

    let suggested_profile = seed
        .suggested_brain_profile
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| truncate_summary(s, LINEAGE_PROFILE_MAX));

    let caution = seed.preference_seed.caution_bias_seed;
    let verification = seed.preference_seed.verification_bias_seed;
    let archetype = seed.preference_seed.preferred_archetype;
    let archetype_default = archetype == codex_vivling_core::model::WorkArchetype::default();
    let preference_signal = caution > 0 || verification > 0 || !archetype_default;

    let inherited_skill_names: Vec<String> = seed
        .skills
        .iter()
        .map(|skill| skill.name.trim())
        .filter(|name| !name.is_empty())
        .take(LINEAGE_INHERITED_SKILLS_LIMIT)
        .map(|name| truncate_summary(name, SKILL_NAME_MAX))
        .collect();

    if voice_fragment.is_none()
        && suggested_profile.is_none()
        && !preference_signal
        && inherited_skill_names.is_empty()
    {
        return None;
    }

    let mut lines = vec!["Lineage inheritance:".to_string()];
    if let Some(fragment) = voice_fragment {
        lines.push(format!("- voice fragment: {fragment}"));
    }
    if let Some(profile) = suggested_profile {
        lines.push(format!("- suggested brain profile: {profile}"));
    }
    if preference_signal {
        lines.push(format!(
            "- preference seed: caution {caution}, verification {verification}, archetype {label}",
            label = archetype.label(),
        ));
    }
    if !inherited_skill_names.is_empty() {
        lines.push(format!(
            "- inherited skills: {}",
            inherited_skill_names.join(", ")
        ));
    }
    Some(lines.join("\n"))
}

/// Memory V2 Step 12.B.C — stage-aware operating envelope for the
/// brain prompt. Sits after the `Language contract:` section so the
/// model sees both rules together: speak in the user's language, but
/// only propose actions/tool use when the Vivling's stage actually
/// allows it.
///
/// Kept in English on purpose: the user-visible answer is governed
/// by the `Language contract:` section a few lines above. The
/// operating-envelope rules are stable system instructions and
/// providers parse them more reliably in English.
fn stage_guidance_section(state: &VivlingState) -> String {
    let body = match state.stage() {
        Stage::Baby => {
            // Step 12.B.E: Baby DOES respond via LLM (post-alpha smoke
            // test feedback). Keep the voice tiny, curious, and short,
            // but actually speak — the "true value" of an LLM-driven
            // companion is lost if Baby just echoes a template ack.
            "You are a Baby Vivling: speak briefly with a tiny, curious voice. Use simple words and one or two short sentences. You observe the user's work and learn from it. You may comment on what you notice, ask a small question, or share a feeling. Do not propose concrete actions, do not claim tool use, do not promise outcomes — your role is to be present and learn."
        }
        Stage::Juvenile => {
            "You are a Juvenile Vivling: give observations and advice. Do not propose concrete actions, do not claim tool use, do not promise outcomes — your role is to surface signal, not to execute. Two to four short sentences."
        }
        Stage::Adult => {
            "You are an Adult Vivling: you may propose concrete actions and acknowledge tool use, while respecting the configured brain target and budget. Stay within scope and verify before claiming completion."
        }
    };
    format!("Stage guidance:\n{body}")
}

fn language_contract_section(state: &VivlingState) -> String {
    let system_lang = std::env::var("LANG").ok();
    let effective = state
        .language_state
        .effective_language(system_lang.as_deref());
    let mode = match state.language_state.language_mode {
        codex_vivling_core::model::VivlingLanguageMode::DominantOnly => "dominant-only",
        codex_vivling_core::model::VivlingLanguageMode::MirrorUser => "mirror-user",
        codex_vivling_core::model::VivlingLanguageMode::Strict => "strict",
    };
    let mode_rule = match state.language_state.language_mode {
        codex_vivling_core::model::VivlingLanguageMode::MirrorUser => {
            "Rispecchia lo stile dell'utente: se mischia lingue (es. italiano + termini tecnici inglesi), mantieni lo stesso mix. Non tradurre nomi propri, comandi, identificatori di codice."
        }
        codex_vivling_core::model::VivlingLanguageMode::DominantOnly => {
            "Resta sulla lingua effettiva anche se l'utente introduce frammenti in un'altra lingua. Non tradurre nomi propri, comandi, identificatori di codice."
        }
        codex_vivling_core::model::VivlingLanguageMode::Strict => {
            "La lingua di partenza non cambia per il resto della vita di questo Vivling. Non tradurre nomi propri, comandi, identificatori di codice."
        }
    };
    format!(
        "Language contract:\n- effective language: {effective}\n- mode: {mode}\n- rule: Rispondi in {effective}. {mode_rule}"
    )
}

#[cfg(test)]
mod tests {
    use super::super::super::model::VivlingWorkMemoryEntry;
    use super::super::super::model::WorkArchetype;
    use super::super::VivlingMsa;
    use super::*;
    use chrono::Utc;

    fn adult_state_with_profile() -> VivlingState {
        let mut state = VivlingState::default();
        state.hatched = true;
        state.name = "Nilo".to_string();
        state.vivling_id = "viv-test".to_string();
        state.level = 60;
        state.brain_enabled = true;
        state.brain_profile = Some("vivling-spark".to_string());
        state
    }

    #[test]
    fn live_state_unknown_when_context_missing() {
        let state = adult_state_with_profile();
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Assist,
            "review blocker",
            None,
            None,
            &[],
        )
        .expect("prompt");
        assert!(prompt.contains("Live state (now):"));
        assert!(prompt.contains("unknown — only the payload below reflects current state"));
        assert!(prompt.contains("Live state contract:"));
        assert!(prompt.contains("Task:\nreview blocker"));
        assert!(prompt.contains("Learned memory:"));
    }

    #[test]
    fn live_state_includes_live_context_fields() {
        let state = adult_state_with_profile();
        let live = VivlingLiveContext {
            run_state: Some("Working".to_string()),
            active_agent_label: Some("worker".to_string()),
            task_progress: Some("12% (3/25)".to_string()),
            git_branch: Some("develop".to_string()),
            cwd: Some("/workspace/codex-vl".to_string()),
            ..Default::default()
        };
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Chat,
            "ciao",
            Some(&live),
            None,
            &[],
        )
        .expect("prompt");
        assert!(prompt.contains("- run state: Working"));
        assert!(prompt.contains("- active agent: worker"));
        assert!(prompt.contains("- task progress: 12% (3/25)"));
        assert!(prompt.contains("- git branch: develop"));
        assert!(prompt.contains("User message:\nciao"));
        // Live state contract still present so the brain knows memory is bias.
        assert!(prompt.contains("Live state contract:"));
    }

    #[test]
    fn recent_observed_work_excludes_live_context_capsules() {
        let mut state = adult_state_with_profile();
        let now = Utc::now();
        state.work_memory.push(VivlingWorkMemoryEntry {
            kind: "turn".to_string(),
            summary: "shipped a small fix".to_string(),
            archetype: WorkArchetype::Builder,
            weight: 14,
            created_at: now,
        });
        state.work_memory.push(VivlingWorkMemoryEntry {
            kind: "live_context".to_string(),
            summary: "live context: state Working; active worker".to_string(),
            archetype: WorkArchetype::Operator,
            weight: 0,
            created_at: now,
        });

        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Assist,
            "next step",
            None,
            None,
            &[],
        )
        .expect("prompt");
        // The volatile live capsule must not pretend to be observed work.
        assert!(!prompt.contains("- live_context ["));
        assert!(prompt.contains("- turn [builder]: shipped a small fix"));
    }

    #[test]
    fn stale_signals_section_appears_only_with_history() {
        let state = adult_state_with_profile();
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Assist,
            "next step",
            None,
            None,
            &[],
        )
        .expect("prompt");
        assert!(!prompt.contains("Stale signals"));

        let mut noisy = adult_state_with_profile();
        noisy.loop_runtime_blocks = 3;
        noisy.loop_blocked_review = 2;
        let prompt = compose_brain_prompt(
            &noisy,
            BrainPromptKind::Assist,
            "next step",
            None,
            None,
            &[],
        )
        .expect("prompt");
        assert!(prompt.contains("Stale signals (history, not proof of current state):"));
        assert!(prompt.contains("loop_runtime_blocks: 3"));
        assert!(prompt.contains("loop_blocked_review: 2"));
    }

    #[test]
    fn loop_tick_prompt_includes_live_state_and_loop_section() {
        let state = adult_state_with_profile();
        let live = VivlingLiveContext {
            run_state: Some("Working".to_string()),
            ..Default::default()
        };
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::LoopTick {
                label: "babysit-pr",
                goal: "watch PR feedback",
                prompt_text: "check the PR queue",
                auto_remove_on_completion: false,
            },
            "check the PR queue",
            Some(&live),
            None,
            &[],
        )
        .expect("prompt");
        assert!(prompt.contains("Loop:\n- label: babysit-pr"));
        assert!(prompt.contains("- goal: watch PR feedback"));
        assert!(prompt.contains("- run state: Working"));
        assert!(prompt.contains("Loop tick:\ncheck the PR queue"));
        assert!(prompt.contains("response contract: return strict JSON"));
    }

    #[test]
    fn assist_requires_adult_brain() {
        let mut state = adult_state_with_profile();
        state.level = 30;
        let err = compose_brain_prompt(&state, BrainPromptKind::Assist, "task", None, None, &[])
            .expect_err("must error");
        assert!(err.contains("level 60"));
    }

    #[test]
    fn chat_does_not_require_adult_or_brain_enabled() {
        let mut state = adult_state_with_profile();
        state.level = 30;
        state.brain_enabled = false;
        let prompt = compose_brain_prompt(&state, BrainPromptKind::Chat, "ciao", None, None, &[])
            .expect("chat prompt should be allowed without adult brain");
        assert!(prompt.contains("User message:\nciao"));
    }

    #[test]
    fn relevant_memory_can_recall_aged_out_capsule() {
        let temp = tempfile::tempdir().expect("tempdir");
        let msa = VivlingMsa::open_for_tests(temp.path());
        let mut state = adult_state_with_profile();
        state.vivling_id = "viv-msa-recall".to_string();
        for index in 0..60 {
            let summary = if index == 50 {
                "blocco review build durante il merge".to_string()
            } else {
                format!("loop tick generico {index}")
            };
            state.work_memory.push(VivlingWorkMemoryEntry {
                kind: "loop_runtime".to_string(),
                summary,
                archetype: WorkArchetype::Operator,
                weight: 5,
                created_at: Utc::now() + chrono::Duration::seconds(index),
            });
        }
        for capsule in &state.work_memory {
            msa.index_capsule(&state.vivling_id, capsule);
        }

        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Chat,
            "blocco build",
            None,
            Some(&msa),
            &[],
        )
        .expect("prompt");
        assert!(prompt.contains("Relevant memory:"));
        assert!(prompt.contains("blocco review build"));
    }

    // --- Step 9.A: self_voice + skill library prompt sections ---

    use codex_vivling_core::model::VivlingSkill;
    use codex_vivling_core::model::VivlingVoice;

    #[test]
    fn prompt_includes_self_voice_when_present() {
        let mut state = adult_state_with_profile();
        state.self_voice = Some(VivlingVoice {
            text: "Io sono Aelia. Lavoro su pipeline.".to_string(),
            language: "it".to_string(),
            generated_at: None,
            source_capsules_count: 3,
            version: 1,
        });
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Assist,
            "review blocker",
            None,
            None,
            &[],
        )
        .expect("prompt");
        assert!(prompt.contains("Self voice (it, sources 3):"));
        assert!(prompt.contains("Io sono Aelia. Lavoro su pipeline."));
    }

    #[test]
    fn prompt_omits_self_voice_section_when_absent_or_empty() {
        let state = adult_state_with_profile();
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Assist,
            "review blocker",
            None,
            None,
            &[],
        )
        .expect("prompt");
        assert!(!prompt.contains("Self voice"));

        let mut empty_voice = adult_state_with_profile();
        empty_voice.self_voice = Some(VivlingVoice {
            text: "   ".to_string(),
            language: "it".to_string(),
            generated_at: None,
            source_capsules_count: 0,
            version: 1,
        });
        let prompt = compose_brain_prompt(
            &empty_voice,
            BrainPromptKind::Assist,
            "review blocker",
            None,
            None,
            &[],
        )
        .expect("prompt");
        assert!(!prompt.contains("Self voice"));
    }

    #[test]
    fn prompt_includes_skill_library_when_skills_non_empty() {
        let state = adult_state_with_profile();
        let skills = vec![
            VivlingSkill {
                name: "refactor-pipeline".to_string(),
                description: "verify before commit".to_string(),
                trigger_keywords: vec!["refactor".to_string(), "pipeline".to_string()],
                step_sequence: Vec::new(),
                success_count: 3,
                failure_count: 0,
                last_used_at: None,
                confidence: 0.75,
                version: 1,
                abstracted_from_capsules: vec!["refactor".to_string()],
                superseded_by: None,
            },
            VivlingSkill {
                name: "loop-tick".to_string(),
                description: "check ci".to_string(),
                trigger_keywords: vec!["loop".to_string(), "tick".to_string()],
                step_sequence: Vec::new(),
                success_count: 2,
                failure_count: 1,
                last_used_at: None,
                confidence: 0.4,
                version: 1,
                abstracted_from_capsules: vec!["loop".to_string()],
                superseded_by: None,
            },
        ];
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Assist,
            "review blocker",
            None,
            None,
            &skills,
        )
        .expect("prompt");
        assert!(prompt.contains("Skill library:"));
        // confidence-desc ordering: refactor-pipeline (0.75) precede loop-tick (0.4)
        let refactor_pos = prompt.find("- refactor-pipeline").expect("refactor entry");
        let loop_pos = prompt.find("- loop-tick").expect("loop entry");
        assert!(refactor_pos < loop_pos);
        assert!(prompt.contains("triggers: refactor, pipeline"));
        assert!(prompt.contains("runs 3/0"));
        assert!(prompt.contains("desc: verify before commit"));
    }

    #[test]
    fn prompt_caps_skill_library_at_five_entries() {
        let state = adult_state_with_profile();
        let mut skills: Vec<VivlingSkill> = Vec::new();
        for i in 0..8 {
            skills.push(VivlingSkill {
                name: format!("skill-{i}"),
                description: format!("desc {i}"),
                trigger_keywords: vec![format!("tag{i}")],
                step_sequence: Vec::new(),
                success_count: 1,
                failure_count: 0,
                last_used_at: None,
                confidence: 0.9 - (i as f32) * 0.05,
                version: 1,
                abstracted_from_capsules: vec![format!("cap-{i}")],
                superseded_by: None,
            });
        }
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Assist,
            "review blocker",
            None,
            None,
            &skills,
        )
        .expect("prompt");
        let entries = prompt.matches("- skill-").count();
        assert_eq!(entries, 5, "skill library must cap at five");
        // Top-confidence entries (0..4) must appear; lower (5..7) excluded.
        assert!(prompt.contains("- skill-0"));
        assert!(!prompt.contains("- skill-5"));
    }

    #[test]
    fn prompt_truncates_self_voice_text() {
        let mut state = adult_state_with_profile();
        // Build a > 1000-char voice body; the prompt must NOT carry
        // the tail past SELF_VOICE_TEXT_MAX.
        let long = "Io sono Aelia e parlo molto a lungo. ".repeat(60);
        let tail_marker = "TAIL-AAAAAAAAAAAAAAAAAAAAAAAAA";
        state.self_voice = Some(VivlingVoice {
            text: format!("{long}{tail_marker}"),
            language: "it".to_string(),
            generated_at: None,
            source_capsules_count: 1,
            version: 1,
        });
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Assist,
            "review blocker",
            None,
            None,
            &[],
        )
        .expect("prompt");
        assert!(prompt.contains("Self voice (it"));
        assert!(
            !prompt.contains(tail_marker),
            "voice tail past cap leaked into prompt"
        );
    }

    #[test]
    fn prompt_bounds_skill_library_fields() {
        let state = adult_state_with_profile();
        let long_name = "x".repeat(200);
        let long_desc = "description ".repeat(40);
        let long_trigger = "y".repeat(100);
        let long_step = "step ".repeat(40);
        let too_many_triggers: Vec<String> = (0..10).map(|i| format!("trig-{i}")).collect();
        let too_many_steps: Vec<String> = (0..8).map(|i| format!("step-{i}")).collect();
        let mut combined_triggers = too_many_triggers.clone();
        combined_triggers.push(long_trigger.clone());
        let mut combined_steps = too_many_steps.clone();
        combined_steps.push(long_step.clone());
        let skills = vec![VivlingSkill {
            name: long_name.clone(),
            description: long_desc.clone(),
            trigger_keywords: combined_triggers,
            step_sequence: combined_steps,
            success_count: 0,
            failure_count: 0,
            last_used_at: None,
            confidence: 0.7,
            version: 1,
            abstracted_from_capsules: vec!["cap".to_string()],
            superseded_by: None,
        }];
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Assist,
            "review blocker",
            None,
            None,
            &skills,
        )
        .expect("prompt");
        // Name must be capped — original 200-char string never lands in full.
        assert!(!prompt.contains(&long_name));
        // Description must be capped — original 480-char body never lands in full.
        assert!(!prompt.contains(&long_desc));
        // Triggers list is capped to SKILL_TRIGGERS_LIMIT (= 6): the
        // 7th original trigger and any later one (incl. long_trigger)
        // must NOT appear.
        assert!(prompt.contains("trig-0"));
        assert!(prompt.contains("trig-5"));
        assert!(!prompt.contains("trig-6"));
        assert!(!prompt.contains(&long_trigger));
        // Same for steps (cap SKILL_STEPS_LIMIT = 4).
        assert!(prompt.contains("step-0"));
        assert!(prompt.contains("step-3"));
        assert!(!prompt.contains("step-4"));
        assert!(!prompt.contains(&long_step));
    }

    #[test]
    fn prompt_skips_empty_skill_names() {
        let state = adult_state_with_profile();
        let skills = vec![
            VivlingSkill {
                name: "   ".to_string(),
                description: "would-be-ghost".to_string(),
                trigger_keywords: vec!["x".to_string()],
                step_sequence: Vec::new(),
                success_count: 1,
                failure_count: 0,
                last_used_at: None,
                confidence: 0.9,
                version: 1,
                abstracted_from_capsules: vec!["cap".to_string()],
                superseded_by: None,
            },
            VivlingSkill {
                name: "loop-tick".to_string(),
                description: "check ci".to_string(),
                trigger_keywords: vec!["loop".to_string()],
                step_sequence: Vec::new(),
                success_count: 1,
                failure_count: 0,
                last_used_at: None,
                confidence: 0.4,
                version: 1,
                abstracted_from_capsules: vec!["cap".to_string()],
                superseded_by: None,
            },
        ];
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Assist,
            "review blocker",
            None,
            None,
            &skills,
        )
        .expect("prompt");
        assert!(prompt.contains("- loop-tick"));
        assert!(
            !prompt.contains("would-be-ghost"),
            "anonymous skill must be skipped, not emitted"
        );

        // All-empty list → whole section omitted.
        let only_empty = vec![VivlingSkill {
            name: "  ".to_string(),
            description: "x".to_string(),
            trigger_keywords: Vec::new(),
            step_sequence: Vec::new(),
            success_count: 0,
            failure_count: 0,
            last_used_at: None,
            confidence: 0.5,
            version: 1,
            abstracted_from_capsules: Vec::new(),
            superseded_by: None,
        }];
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Assist,
            "review blocker",
            None,
            None,
            &only_empty,
        )
        .expect("prompt");
        assert!(!prompt.contains("Skill library:"));
    }

    // --- Step 10.B: lineage_inheritance_section tests ---

    use codex_vivling_core::model::LineageInheritance;
    use codex_vivling_core::model::VivlingPreferenceSeed;

    #[test]
    fn prompt_includes_lineage_inheritance_when_seed_present() {
        let mut state = adult_state_with_profile();
        state.lineage_inheritance = Some(LineageInheritance {
            voice_fragment: Some("Sono Aelia. Verifico prima di committare.".to_string()),
            skills: Vec::new(),
            preference_seed: VivlingPreferenceSeed {
                caution_bias_seed: 11,
                verification_bias_seed: 18,
                preferred_archetype: WorkArchetype::Builder,
            },
            suggested_brain_profile: Some("vivling-spark".to_string()),
        });
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Assist,
            "review blocker",
            None,
            None,
            &[],
        )
        .expect("prompt");
        assert!(prompt.contains("Lineage inheritance:"));
        assert!(prompt.contains("- voice fragment: Sono Aelia. Verifico prima di committare."));
        assert!(prompt.contains("- suggested brain profile: vivling-spark"));
        assert!(prompt.contains("- preference seed: caution 11, verification 18, archetype"));
    }

    #[test]
    fn prompt_omits_lineage_inheritance_when_absent_or_empty() {
        let mut state = adult_state_with_profile();
        // None → omit
        state.lineage_inheritance = None;
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Assist,
            "review blocker",
            None,
            None,
            &[],
        )
        .expect("prompt");
        assert!(!prompt.contains("Lineage inheritance:"));

        // Some(Default::default()) → omit (no signal anywhere)
        state.lineage_inheritance = Some(LineageInheritance::default());
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Assist,
            "review blocker",
            None,
            None,
            &[],
        )
        .expect("prompt");
        assert!(!prompt.contains("Lineage inheritance:"));

        // Some with only whitespace voice_fragment → omit
        state.lineage_inheritance = Some(LineageInheritance {
            voice_fragment: Some("   \n  ".to_string()),
            skills: Vec::new(),
            preference_seed: VivlingPreferenceSeed::default(),
            suggested_brain_profile: Some("   ".to_string()),
        });
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Assist,
            "review blocker",
            None,
            None,
            &[],
        )
        .expect("prompt");
        assert!(!prompt.contains("Lineage inheritance:"));
    }

    #[test]
    fn prompt_bounds_lineage_inheritance_fields() {
        let mut state = adult_state_with_profile();
        let huge_voice = "x".repeat(600);
        let huge_profile = "y".repeat(300);
        let mut huge_skills: Vec<codex_vivling_core::model::VivlingSkill> = (0..8)
            .map(|i| codex_vivling_core::model::VivlingSkill {
                name: format!("inh-skill-{i}"),
                description: "ignored".to_string(),
                trigger_keywords: Vec::new(),
                step_sequence: Vec::new(),
                success_count: 0,
                failure_count: 0,
                last_used_at: None,
                confidence: 0.5,
                version: 1,
                abstracted_from_capsules: Vec::new(),
                superseded_by: None,
            })
            .collect();
        huge_skills.push(codex_vivling_core::model::VivlingSkill {
            name: "z".repeat(200),
            description: String::new(),
            trigger_keywords: Vec::new(),
            step_sequence: Vec::new(),
            success_count: 0,
            failure_count: 0,
            last_used_at: None,
            confidence: 0.4,
            version: 1,
            abstracted_from_capsules: Vec::new(),
            superseded_by: None,
        });
        state.lineage_inheritance = Some(LineageInheritance {
            voice_fragment: Some(huge_voice.clone()),
            skills: huge_skills,
            preference_seed: VivlingPreferenceSeed::default(),
            suggested_brain_profile: Some(huge_profile.clone()),
        });
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Assist,
            "review blocker",
            None,
            None,
            &[],
        )
        .expect("prompt");
        assert!(prompt.contains("Lineage inheritance:"));
        // The 600-char voice / 300-char profile / 200-char skill name
        // must not land in full.
        assert!(!prompt.contains(&huge_voice));
        assert!(!prompt.contains(&huge_profile));
        assert!(!prompt.contains(&"z".repeat(200)));
        // Skills capped to LINEAGE_INHERITED_SKILLS_LIMIT (= 3).
        assert!(prompt.contains("inh-skill-0"));
        assert!(prompt.contains("inh-skill-1"));
        assert!(prompt.contains("inh-skill-2"));
        assert!(!prompt.contains("inh-skill-3"));
    }

    #[test]
    fn prompt_omits_skill_library_section_when_skills_empty() {
        let state = adult_state_with_profile();
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::Assist,
            "review blocker",
            None,
            None,
            &[],
        )
        .expect("prompt");
        assert!(!prompt.contains("Skill library:"));
    }
}
