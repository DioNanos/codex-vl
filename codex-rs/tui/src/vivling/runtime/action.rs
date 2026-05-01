use super::*;

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
}
