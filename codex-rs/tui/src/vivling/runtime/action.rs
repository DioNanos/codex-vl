use super::*;

/// Memory V2 Step 12.B.D.3 — sub-actions for `/vivling crt-brain`.
/// Tri-state opt-in (`Default`/`On`/`Off`) governs the live
/// Expression channel (CRT footer phrase + proactive). `Show` prints
/// the current mode + today's LLM call counters.
#[derive(Debug, PartialEq)]
pub(crate) enum CrtBrainAction {
    /// `/vivling crt-brain` — print mode + today's counters.
    Show,
    /// `/vivling crt-brain on` — force the channel on regardless of stage.
    On,
    /// `/vivling crt-brain off` — mute the channel entirely.
    Off,
    /// `/vivling crt-brain default` — stage-driven (Adult/Juvenile run,
    /// Baby rare-event only).
    Default,
}

/// Memory V2 §8.2 (Step 5.B) — sub-actions for `/vivling language`.
/// Kept on a dedicated enum so the `VivlingAction` surface does not
/// sprout four new top-level variants.
#[derive(Debug, PartialEq)]
pub(crate) enum LanguageAction {
    /// `/vivling language` — show current effective / detected / override / mode.
    Show,
    /// `/vivling language auto` (or `clear`) — drop the explicit override.
    Auto,
    /// `/vivling language <code>` — pin one of the supported language codes.
    Set(String),
    /// `/vivling language mode <mirror-user|dominant-only|strict>`.
    Mode(String),
}

#[derive(Debug, PartialEq)]
pub(crate) enum VivlingAction {
    Hatch,
    Dashboard,
    Help,
    Status,
    Roster,
    Focus(String),
    Spawn,
    Export(Option<String>),
    Import(String),
    Remove(String),
    Memory,
    Card,
    Upgrade,
    Assist(String),
    Brain(bool),
    ModelShow,
    ModelList,
    ModelProfile(String),
    ModelCustom {
        model: String,
        provider: Option<String>,
        effort: Option<ReasoningEffortConfig>,
    },
    Recap,
    PromoteEarly,
    PromoteAdult,
    Mode(VivlingAiMode),
    Chat(String),
    DirectMessage(String),
    Reset,
    Zed,
    /// Memory V2 §8.2 — `/vivling language [...]`.
    Language(LanguageAction),
    /// Memory V2 Step 12.B.D.3 — `/vivling crt-brain [...]`.
    CrtBrain(CrtBrainAction),
}

impl VivlingAction {
    pub(crate) fn parse(args: &str) -> Result<Self, String> {
        let trimmed = args.trim();
        if trimmed.is_empty() {
            return Ok(Self::Dashboard);
        }
        if trimmed == "status" {
            return Ok(Self::Status);
        }
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or_default();
        let rest = parts.next().unwrap_or_default().trim();
        match cmd {
            "hatch" => Ok(Self::Hatch),
            "help" => Ok(Self::Help),
            "roster" | "list" => Ok(Self::Roster),
            "focus" | "switch" | "select" | "use" => {
                if rest.is_empty() {
                    Err("Usage: /vivling focus <vivling_id_or_name>".to_string())
                } else {
                    Ok(Self::Focus(rest.to_string()))
                }
            }
            "spawn" => Ok(Self::Spawn),
            "export" => {
                if rest.is_empty() {
                    Ok(Self::Export(None))
                } else {
                    Ok(Self::Export(Some(rest.to_string())))
                }
            }
            "import" => {
                if rest.is_empty() {
                    Err("Usage: /vivling import <path.vivegg>".to_string())
                } else {
                    Ok(Self::Import(rest.to_string()))
                }
            }
            "remove" => {
                if rest.is_empty() {
                    Err("Usage: /vivling remove <vivling_id_or_name>".to_string())
                } else {
                    Ok(Self::Remove(rest.to_string()))
                }
            }
            "memory" => Ok(Self::Memory),
            "recap" => Ok(Self::Recap),
            "card" => Ok(Self::Card),
            "upgrade" => Ok(Self::Upgrade),
            "zed" => Ok(Self::Zed),
            "assist" => {
                if rest.is_empty() {
                    Err("Usage: /vivling assist <task>".to_string())
                } else {
                    Ok(Self::Assist(rest.to_string()))
                }
            }
            "brain" => match rest {
                "on" => Ok(Self::Brain(true)),
                "off" => Ok(Self::Brain(false)),
                _ => Err("Usage: /vivling brain <on|off>".to_string()),
            },
            "model" => Self::parse_model_action(rest),
            "promote" => match rest {
                "10" => Ok(Self::PromoteEarly),
                "60" => Ok(Self::PromoteAdult),
                _ => Err("Usage: /vivling promote <10|60>".to_string()),
            },
            "mode" => VivlingAiMode::parse(rest)
                .map(Self::Mode)
                .ok_or_else(|| "Usage: /vivling mode <on|off>".to_string()),
            "language" => Self::parse_language_action(rest),
            "crt-brain" | "crt_brain" | "crtbrain" => Self::parse_crt_brain_action(rest),
            "reset" => Ok(Self::Reset),
            _ => Ok(Self::DirectMessage(trimmed.to_string())),
        }
    }

    fn parse_model_action(rest: &str) -> Result<Self, String> {
        let trimmed = rest.trim();
        if trimmed.is_empty() {
            return Ok(Self::ModelShow);
        }
        if trimmed.eq_ignore_ascii_case("list") {
            return Ok(Self::ModelList);
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() == 1 {
            return Ok(Self::ModelProfile(parts[0].to_string()));
        }

        let model = parts[0].to_string();
        let mut provider = None;
        let mut effort = None;

        for token in parts.iter().skip(1) {
            if effort.is_none()
                && let Ok(parsed_effort) = token.parse::<ReasoningEffortConfig>()
            {
                effort = Some(parsed_effort);
                continue;
            }
            if provider.is_none() {
                provider = Some((*token).to_string());
                continue;
            }
            return Err(
                "Usage: /vivling model <profile> | /vivling model <model> [provider] [effort]"
                    .to_string(),
            );
        }

        Ok(Self::ModelCustom {
            model,
            provider,
            effort,
        })
    }

    fn parse_crt_brain_action(rest: &str) -> Result<Self, String> {
        let trimmed = rest.trim();
        if trimmed.is_empty() {
            return Ok(Self::CrtBrain(CrtBrainAction::Show));
        }
        match trimmed.to_ascii_lowercase().as_str() {
            "show" | "status" => Ok(Self::CrtBrain(CrtBrainAction::Show)),
            "on" => Ok(Self::CrtBrain(CrtBrainAction::On)),
            "off" => Ok(Self::CrtBrain(CrtBrainAction::Off)),
            "default" | "auto" | "clear" | "reset" => Ok(Self::CrtBrain(CrtBrainAction::Default)),
            _ => Err("Usage: /vivling crt-brain [show|on|off|default]".to_string()),
        }
    }

    fn parse_language_action(rest: &str) -> Result<Self, String> {
        let trimmed = rest.trim();
        if trimmed.is_empty() {
            return Ok(Self::Language(LanguageAction::Show));
        }
        let lower = trimmed.to_ascii_lowercase();
        match lower.as_str() {
            "show" | "status" => return Ok(Self::Language(LanguageAction::Show)),
            "auto" | "clear" | "reset" | "default" => {
                return Ok(Self::Language(LanguageAction::Auto));
            }
            _ => {}
        }
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let head = parts.next().unwrap_or_default();
        let tail = parts.next().unwrap_or_default().trim();
        if head.eq_ignore_ascii_case("mode") {
            if tail.is_empty() {
                return Err(
                    "Usage: /vivling language mode <mirror-user|dominant-only|strict>".to_string(),
                );
            }
            return Ok(Self::Language(LanguageAction::Mode(tail.to_string())));
        }
        // Single token, not a sub-command: treat as a language code.
        if tail.is_empty() {
            return Ok(Self::Language(LanguageAction::Set(head.to_string())));
        }
        Err(
            "Usage: /vivling language [auto|<code>|mode <mirror-user|dominant-only|strict>]"
                .to_string(),
        )
    }
}
