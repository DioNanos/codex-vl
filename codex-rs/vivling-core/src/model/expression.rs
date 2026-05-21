//! Memory V2 Step 12.A — Axis F deterministic expression prompt planner.
//!
//! Moved into `codex-vivling-core` by Step 12.B.0 so the TUI runtime
//! can compose the prompt without taking a dependency on the memory
//! agent crate. The memory agent re-exports these symbols verbatim so
//! the dry-run report shape stays unchanged.
//!
//! Pure: no LLM, no network, no I/O. Same source → same
//! `ExpressionPromptPlan` byte-for-byte for a given `now`.
//!
//! Every input string is normalised through
//! `codex_vivling_core::redaction::redacted_semantic_text` before it
//! reaches the prompt, so a state file made of just `[REDACTED:*]`
//! markers cannot promote noise into the LLM context.
use std::path::PathBuf;

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use super::types::VivlingDistilledSummary;
use super::types::VivlingLanguageState;
use super::types::VivlingWorkMemoryEntry;
use crate::redaction::redacted_semantic_text;

use super::text_utils::truncate_summary;

/// Expression-prompt schema version emitted by [`plan_expression_prompt`].
/// Bumped only when the deterministic prompt template shape changes.
pub const EXPRESSION_PROMPT_VERSION: u32 = 1;

/// Hard cap on the prompt body length, in characters. Step 12.A's whole
/// point is to draft a *bounded* prompt the future Step 12.B can hand to
/// an LLM with confidence; the bound is enforced even across
/// heterogeneous sources so a malicious or huge state file can never
/// blow the LLM budget.
pub const EXPRESSION_PROMPT_MAX_CHARS: usize = 2_000;
pub const EXPRESSION_VOICE_FRAGMENT_MAX: usize = 240;
pub const EXPRESSION_CAPSULE_TEXT_MAX: usize = 96;
pub const EXPRESSION_MAX_CAPSULES: usize = 3;
pub const EXPRESSION_NAME_MAX: usize = 80;
pub const EXPRESSION_NAME_FALLBACK: &str = "Vivling";

/// Header projection consumed by every planner (voice / skill /
/// expression). Kept here in core so all three planners share the
/// same view of the state JSON without re-defining it three times.
#[derive(Debug, Deserialize)]
pub struct PlannerStateProjection {
    #[serde(default)]
    pub vivling_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub hatched: bool,
    #[serde(default)]
    pub language_state: VivlingLanguageState,
    #[serde(default)]
    pub work_memory: Vec<VivlingWorkMemoryEntry>,
    #[serde(default)]
    pub distilled_summaries: Vec<VivlingDistilledSummary>,
}

/// Why the expression planner produced no prompt. Same
/// `serde(snake_case)` shape as the voice/skill skip enums so JSON
/// consumers can render all three uniformly.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionPlanSkipReason {
    NotHatched,
    NoSourceMaterial,
}

impl ExpressionPlanSkipReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExpressionPlanSkipReason::NotHatched => "not hatched yet",
            ExpressionPlanSkipReason::NoSourceMaterial => "no source material",
        }
    }
}

/// Where the expression planner pulled its anchor text from. The
/// drafted prompt mixes both layers when available; the field reports
/// the *primary* source so consumers can render confidence / freshness
/// without re-parsing the prompt.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionPlanPrimarySource {
    SelfVoice,
    DistilledSummaries,
    WorkMemoryCapsules,
}

/// Output of [`plan_expression_prompt`]. The `prompt` is bounded and
/// deterministic; Step 12.B+ would feed it into the configured LLM
/// when the Vivling needs to express itself.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ExpressionPromptPlan {
    pub prompt: String,
    pub language: String,
    pub primary_source: ExpressionPlanPrimarySource,
    pub sources_count: usize,
    pub generated_at: DateTime<Utc>,
    pub version: u32,
}

/// Error returned by the planner when the input state JSON cannot be
/// parsed. Mirrors `MemoryAgentError::InvalidStateJson` but lives in
/// core so callers do not need to depend on the memory-agent crate.
#[derive(Debug, thiserror::Error)]
#[error("expression planner could not parse state body at {path}: {source}")]
pub struct ExpressionPlanParseError {
    pub path: PathBuf,
    #[source]
    pub source: serde_json::Error,
}

/// Plan a Vivling expression prompt from a state JSON body.
///
/// Source priority: `self_voice` (when non-empty after redaction),
/// then the top-N distilled summaries, then the most recent
/// work-memory capsules as a fallback. All text is run through
/// `redacted_semantic_text` so a sidecar made of just `[REDACTED:*]`
/// markers cannot promote noise into the LLM prompt.
///
/// Determinism contract: for a given `body` and `now`, returns the
/// same `ExpressionPromptPlan` byte-for-byte. No LLM, no randomness,
/// no environment lookups.
pub fn plan_expression_prompt(
    body: &str,
    now: DateTime<Utc>,
) -> Result<Result<ExpressionPromptPlan, ExpressionPlanSkipReason>, ExpressionPlanParseError> {
    let projection: PlannerStateProjection =
        serde_json::from_str(body).map_err(|err| ExpressionPlanParseError {
            path: PathBuf::from("<in-memory>"),
            source: err,
        })?;

    if !projection.hatched {
        return Ok(Err(ExpressionPlanSkipReason::NotHatched));
    }

    let language = projection.language_state.effective_language(None);
    // Step 12.A round-2 fix: name is redacted and bounded so a state
    // file with a secret in `name` cannot leak into the LLM prompt —
    // and a `name` longer than the prompt budget cannot crowd out the
    // rest of the expression.
    let name_display = expression_display_name(&projection);

    // Anchor #1: a previously written self_voice (Step 7.B), bounded
    // and only if the redacted text carries real content.
    let voice_anchor: Option<String> = (|| {
        let voice = projection_self_voice(body)?;
        let bounded = redacted_semantic_text(&voice)
            .map(|text| truncate_summary(text.trim(), EXPRESSION_VOICE_FRAGMENT_MAX))?;
        if bounded.trim().is_empty() {
            None
        } else {
            Some(bounded)
        }
    })();

    let valid_summaries: Vec<&VivlingDistilledSummary> = projection
        .distilled_summaries
        .iter()
        .filter(|s| {
            let topic_ok = redacted_semantic_text(&s.topic).is_some();
            let summary_ok = redacted_semantic_text(&s.summary).is_some();
            let has_signal = s.observations > 0 || s.total_weight > 0;
            (topic_ok || summary_ok) && has_signal
        })
        .collect();
    let valid_capsules: Vec<&VivlingWorkMemoryEntry> = projection
        .work_memory
        .iter()
        .filter(|c| {
            let kind_ok = redacted_semantic_text(&c.kind).is_some();
            let summary_ok = redacted_semantic_text(&c.summary).is_some();
            let has_signal = c.weight > 0;
            (kind_ok || summary_ok) && has_signal
        })
        .collect();

    if voice_anchor.is_none() && valid_summaries.is_empty() && valid_capsules.is_empty() {
        return Ok(Err(ExpressionPlanSkipReason::NoSourceMaterial));
    }

    let primary_source = if voice_anchor.is_some() {
        ExpressionPlanPrimarySource::SelfVoice
    } else if !valid_summaries.is_empty() {
        ExpressionPlanPrimarySource::DistilledSummaries
    } else {
        ExpressionPlanPrimarySource::WorkMemoryCapsules
    };

    let mut prompt_lines: Vec<String> = Vec::new();
    prompt_lines.push(format!("You are {name_display}. Speak in {language}."));
    if let Some(voice) = voice_anchor.as_deref() {
        prompt_lines.push(format!("Your established voice: {voice}"));
    }
    let mut sources_count: usize = if voice_anchor.is_some() { 1 } else { 0 };

    let mut summaries = valid_summaries;
    summaries.sort_by(|a, b| {
        b.total_weight
            .cmp(&a.total_weight)
            .then_with(|| a.topic.cmp(&b.topic))
    });
    let mut capsules = valid_capsules;
    capsules.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let mut anchor_lines: Vec<String> = Vec::new();
    for summary in summaries.into_iter().take(EXPRESSION_MAX_CAPSULES) {
        let topic = redacted_semantic_text(&summary.topic)
            .map(|t| truncate_summary(t.trim(), EXPRESSION_CAPSULE_TEXT_MAX))
            .unwrap_or_default();
        let pattern = redacted_semantic_text(&summary.summary)
            .map(|p| truncate_summary(p.trim(), EXPRESSION_CAPSULE_TEXT_MAX))
            .unwrap_or_default();
        if topic.is_empty() && pattern.is_empty() {
            continue;
        }
        anchor_lines.push(format!("- {topic}: {pattern}"));
        sources_count += 1;
    }
    if anchor_lines.is_empty() && voice_anchor.is_none() {
        for capsule in capsules.into_iter().take(EXPRESSION_MAX_CAPSULES) {
            let topic = redacted_semantic_text(&capsule.kind)
                .map(|t| truncate_summary(t.trim(), EXPRESSION_CAPSULE_TEXT_MAX))
                .unwrap_or_default();
            let pattern = redacted_semantic_text(&capsule.summary)
                .map(|p| truncate_summary(p.trim(), EXPRESSION_CAPSULE_TEXT_MAX))
                .unwrap_or_default();
            if topic.is_empty() && pattern.is_empty() {
                continue;
            }
            anchor_lines.push(format!("- {topic}: {pattern}"));
            sources_count += 1;
        }
    }
    if !anchor_lines.is_empty() {
        prompt_lines.push("Recent patterns:".to_string());
        prompt_lines.extend(anchor_lines);
    }

    if sources_count == 0 {
        return Ok(Err(ExpressionPlanSkipReason::NoSourceMaterial));
    }

    let mut prompt = prompt_lines.join("\n");
    if prompt.chars().count() > EXPRESSION_PROMPT_MAX_CHARS {
        prompt = truncate_summary(&prompt, EXPRESSION_PROMPT_MAX_CHARS);
    }

    Ok(Ok(ExpressionPromptPlan {
        prompt,
        language,
        primary_source,
        sources_count,
        generated_at: now,
        version: EXPRESSION_PROMPT_VERSION,
    }))
}

/// Extract the `self_voice.text` field from a raw state JSON body
/// without forcing the planner to depend on the full `VivlingState`.
/// Returns `None` when the field is absent, null, or carries an empty
/// `text`.
fn projection_self_voice(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let text = value
        .get("self_voice")?
        .get("text")?
        .as_str()?
        .trim()
        .to_string();
    if text.is_empty() { None } else { Some(text) }
}

/// Step 12.A round-2 fix — redact + bound the Vivling's display name
/// before it lands in the expression prompt.
///
/// Resolution order:
/// 1. `name`, after `redacted_semantic_text` + trim + cap.
/// 2. `vivling_id`, same treatment, when `name` collapses.
/// 3. Static `EXPRESSION_NAME_FALLBACK` when both fields are missing,
///    empty after redaction, or made entirely of redaction markers.
///
/// Never returns a raw secret and never returns a string longer than
/// `EXPRESSION_NAME_MAX`. Marker-only names cannot become a Vivling's
/// identity: that role goes to the fallback so the LLM still has a
/// usable handle.
fn expression_display_name(projection: &PlannerStateProjection) -> String {
    if let Some(name) = redacted_semantic_text(&projection.name) {
        let bounded = truncate_summary(name.trim(), EXPRESSION_NAME_MAX);
        if !bounded.trim().is_empty() {
            return bounded;
        }
    }
    if let Some(id) = redacted_semantic_text(&projection.vivling_id) {
        let bounded = truncate_summary(id.trim(), EXPRESSION_NAME_MAX);
        if !bounded.trim().is_empty() {
            return bounded;
        }
    }
    EXPRESSION_NAME_FALLBACK.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-05-21T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn planner_skips_unhatched() {
        let body = r#"{"vivling_id":"viv-1","name":"Aelia","hatched":false}"#;
        let outcome = plan_expression_prompt(body, make_now()).expect("parse");
        assert_eq!(outcome, Err(ExpressionPlanSkipReason::NotHatched));
    }

    #[test]
    fn planner_skips_empty_state() {
        let body = r#"{"vivling_id":"viv-1","name":"Aelia","hatched":true}"#;
        let outcome = plan_expression_prompt(body, make_now()).expect("parse");
        assert_eq!(outcome, Err(ExpressionPlanSkipReason::NoSourceMaterial));
    }

    #[test]
    fn planner_is_deterministic_for_same_input() {
        let body = r#"{
            "vivling_id":"viv-1",
            "name":"Aelia",
            "hatched":true,
            "self_voice":{"text":"Io sono Aelia","language":"it","source_capsules_count":1,"version":1}
        }"#;
        let a = plan_expression_prompt(body, make_now())
            .expect("parse")
            .expect("plan");
        let b = plan_expression_prompt(body, make_now())
            .expect("parse")
            .expect("plan");
        assert_eq!(a, b);
    }
}
