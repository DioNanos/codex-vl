//! Memory V2 §8.2 Step 5.B — `/vivling language` command surface and
//! generic-turn sampling hook.
//!
//! All persistence goes through `save_state` so the language preferences
//! survive across sessions. The CLI accepts cases the user actually types
//! (lowercased, with hyphen or no hyphen for modes), and rejects anything
//! that isn't one of the supported codes with a precise error pointing at
//! `SUPPORTED_LANGS`.

use super::super::*;
use codex_vivling_core::model::SUPPORTED_LANGS;
use codex_vivling_core::model::VivlingLanguageMode;

impl Vivling {
    pub(crate) fn show_language_status(&mut self) -> Result<String, String> {
        self.ensure_hatched()?;
        let state = self.state.as_ref().expect("state checked");
        let system_lang = std::env::var("LANG").ok();
        let effective = state
            .language_state
            .effective_language(system_lang.as_deref());
        let detected = if state.language_state.detected_language.is_empty() {
            "(none yet)".to_string()
        } else {
            state.language_state.detected_language.clone()
        };
        let override_ = state
            .language_state
            .language_override
            .clone()
            .unwrap_or_else(|| "(none)".to_string());
        let mode = match state.language_state.language_mode {
            VivlingLanguageMode::DominantOnly => "dominant-only",
            VivlingLanguageMode::MirrorUser => "mirror-user",
            VivlingLanguageMode::Strict => "strict",
        };
        let sample_count = state.language_state.recent_samples.len();
        let supported = SUPPORTED_LANGS.join(", ");
        Ok(format!(
            "Vivling language\n- effective: {effective}\n- detected: {detected}\n- override: {override_}\n- mode: {mode}\n- samples: {sample_count}\n- supported codes: {supported}"
        ))
    }

    pub(crate) fn set_language_override(
        &mut self,
        value: Option<String>,
    ) -> Result<String, String> {
        self.ensure_hatched()?;
        if let Some(ref code) = value {
            let normalised = code.trim().to_ascii_lowercase();
            if !SUPPORTED_LANGS.contains(&normalised.as_str()) {
                return Err(format!(
                    "Unsupported language code `{code}`. Pick one of: {}.",
                    SUPPORTED_LANGS.join(", ")
                ));
            }
            self.update_existing_result(|state| {
                state.language_state.language_override = Some(normalised.clone());
                Ok(format!("Vivling language override set to `{normalised}`."))
            })
        } else {
            self.update_existing_result(|state| {
                state.language_state.language_override = None;
                Ok(
                    "Vivling language override cleared; reverting to detected language and LANG fallback."
                        .to_string(),
                )
            })
        }
    }

    pub(crate) fn set_language_mode(&mut self, mode_str: &str) -> Result<String, String> {
        self.ensure_hatched()?;
        let mode = parse_language_mode(mode_str).ok_or_else(|| {
            "Usage: /vivling language mode <mirror-user|dominant-only|strict>".to_string()
        })?;
        self.update_existing_result(|state| {
            state.language_state.language_mode = mode;
            // Refresh so a fresh Strict latches on whatever is currently
            // the dominant detection (instead of waiting for the next sample).
            state.language_state.refresh_detected_language();
            let label = match mode {
                VivlingLanguageMode::DominantOnly => "dominant-only",
                VivlingLanguageMode::MirrorUser => "mirror-user",
                VivlingLanguageMode::Strict => "strict",
            };
            Ok(format!("Vivling language mode set to `{label}`."))
        })
    }

    /// Best-effort sampling hook for ordinary user turns coming from the
    /// chatwidget. Never panics: failure to update or save is logged at
    /// debug level and swallowed, because the language window is a
    /// non-critical signal and must not break the user's normal flow.
    pub(crate) fn record_user_language_sample(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        if !self.state.as_ref().is_some_and(|state| state.hatched) {
            return;
        }
        let now = Utc::now();
        if let Some(state) = self.state.as_mut() {
            state.language_state.record_sample(now, text);
            state.language_state.refresh_detected_language();
        }
        if let Err(err) = self.save_state() {
            tracing::debug!(
                target: "vivling::language",
                "failed to persist language sample: {err}"
            );
        }
    }
}

fn parse_language_mode(raw: &str) -> Option<VivlingLanguageMode> {
    let trimmed = raw.trim().to_ascii_lowercase();
    match trimmed.as_str() {
        "mirror" | "mirror-user" | "mirror_user" => Some(VivlingLanguageMode::MirrorUser),
        "dominant" | "dominant-only" | "dominant_only" => Some(VivlingLanguageMode::DominantOnly),
        "strict" => Some(VivlingLanguageMode::Strict),
        _ => None,
    }
}
