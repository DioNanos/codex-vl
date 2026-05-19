//! codex-vl loop_controller: input parsers.
//!
//! Pure parsing utilities: command JSON / interval tokens / Vivling
//! status strings / `manage_loops` dynamic tool requests.

use crate::vl::events::LoopCommandRequest;
use crate::vl::loop_runtime::LoopJobPayload;

use super::formatting::LOOP_STATUS_BLOCKED;
use super::formatting::LOOP_STATUS_DONE;
use super::formatting::LOOP_STATUS_PROGRESS;

pub(super) const MANAGE_LOOPS_TOOL_NAMESPACE: &str = "codex_app";
pub(super) const MANAGE_LOOPS_TOOL_NAME: &str = "manage_loops";

pub(super) fn is_manage_loops_dynamic_tool(namespace: Option<&str>, tool: &str) -> bool {
    matches!(
        namespace,
        None | Some(MANAGE_LOOPS_TOOL_NAMESPACE) | Some("functions")
    ) && tool == MANAGE_LOOPS_TOOL_NAME
}

#[derive(Debug, serde::Deserialize)]
struct ManageLoopsToolArgs {
    action: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    interval: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    payload: Option<serde_json::Value>,
    #[serde(default)]
    auto_remove_on_completion: Option<bool>,
    #[serde(default)]
    enabled: Option<bool>,
}

pub(super) fn parse_manage_loops_interval_seconds(token: &str) -> Option<i64> {
    if token.len() < 2 {
        return None;
    }
    let (value, unit) = token.split_at(token.len() - 1);
    let value = value.parse::<i64>().ok()?;
    let seconds = match unit {
        "s" => value,
        "m" => value * 60,
        "h" => value * 3600,
        _ => return None,
    };
    ((30..=86_400).contains(&seconds)).then_some(seconds)
}

pub(super) fn parse_vivling_loop_status(status: &str) -> anyhow::Result<&'static str> {
    match status.trim().to_ascii_lowercase().as_str() {
        LOOP_STATUS_PROGRESS => Ok(LOOP_STATUS_PROGRESS),
        LOOP_STATUS_BLOCKED => Ok(LOOP_STATUS_BLOCKED),
        LOOP_STATUS_DONE => Ok(LOOP_STATUS_DONE),
        other => Err(anyhow::anyhow!(
            "Vivling loop tick returned unsupported status `{other}`"
        )),
    }
}

pub(super) fn parse_add_goal(
    raw_goal: Option<serde_json::Value>,
) -> anyhow::Result<Option<String>> {
    match raw_goal {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(goal)) if !goal.trim().is_empty() => Ok(Some(goal)),
        Some(serde_json::Value::String(_)) => {
            Err(anyhow::anyhow!("`goal` cannot be empty when provided"))
        }
        Some(_) => Err(anyhow::anyhow!("`goal` must be a string or null")),
    }
}

pub(super) fn parse_update_goal(
    raw_goal: Option<serde_json::Value>,
) -> anyhow::Result<Option<Option<String>>> {
    match raw_goal {
        None => Ok(None),
        Some(serde_json::Value::Null) => Ok(Some(None)),
        Some(serde_json::Value::String(goal)) if !goal.trim().is_empty() => Ok(Some(Some(goal))),
        Some(serde_json::Value::String(_)) => {
            Err(anyhow::anyhow!("`goal` cannot be empty when provided"))
        }
        Some(_) => Err(anyhow::anyhow!("`goal` must be a string or null")),
    }
}

pub(super) fn parse_manage_loops_tool_request(
    arguments: serde_json::Value,
) -> anyhow::Result<LoopCommandRequest> {
    let goal_argument = arguments
        .as_object()
        .and_then(|object| object.get("goal"))
        .cloned();
    let args: ManageLoopsToolArgs = serde_json::from_value(arguments)?;
    let action = args.action.trim().to_ascii_lowercase();
    match action.as_str() {
        "list" | "ls" => Ok(LoopCommandRequest::List),
        "show" => Ok(LoopCommandRequest::Show {
            label: args
                .label
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`label` is required for show"))?,
        }),
        "enable" | "on" => Ok(LoopCommandRequest::Enable {
            label: args
                .label
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`label` is required for enable"))?,
        }),
        "disable" | "off" => Ok(LoopCommandRequest::Disable {
            label: args
                .label
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`label` is required for disable"))?,
        }),
        "remove" | "rm" => Ok(LoopCommandRequest::Remove {
            label: args
                .label
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`label` is required for remove"))?,
        }),
        "trigger" => Ok(LoopCommandRequest::Trigger {
            label: args
                .label
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`label` is required for trigger"))?,
        }),
        "add" => {
            let label = args
                .label
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`label` is required for add"))?;
            let interval_seconds = parse_manage_loops_interval_seconds(
                args.interval
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("`interval` is required for add"))?,
            )
            .ok_or_else(|| anyhow::anyhow!("`interval` must be between 30s and 24h"))?;
            let payload = LoopJobPayload::from_tool_payload(args.payload, args.prompt)?;
            let prompt_text = payload.to_storage_text()?;
            Ok(LoopCommandRequest::Add {
                label,
                interval_seconds,
                prompt_text,
                goal_text: parse_add_goal(goal_argument)?,
                auto_remove_on_completion: args.auto_remove_on_completion,
            })
        }
        "update" => {
            let label = args
                .label
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`label` is required for update"))?;
            let interval_seconds = match args.interval {
                Some(interval) => Some(
                    parse_manage_loops_interval_seconds(&interval)
                        .ok_or_else(|| anyhow::anyhow!("`interval` must be between 30s and 24h"))?,
                ),
                None => None,
            };
            let prompt_text = match args.payload {
                Some(payload) => Some(
                    LoopJobPayload::from_tool_payload(Some(payload), args.prompt)?
                        .to_storage_text()?,
                ),
                None => match args.prompt {
                    Some(prompt) if !prompt.trim().is_empty() => Some(prompt),
                    Some(_) => {
                        return Err(anyhow::anyhow!("`prompt` cannot be empty when provided"));
                    }
                    None => None,
                },
            };
            Ok(LoopCommandRequest::Update {
                label,
                interval_seconds,
                prompt_text,
                goal_text: parse_update_goal(goal_argument)?,
                auto_remove_on_completion: args.auto_remove_on_completion,
                enabled: args.enabled,
            })
        }
        other => Err(anyhow::anyhow!("unsupported manage_loops action `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::is_manage_loops_dynamic_tool;
    use super::parse_manage_loops_tool_request;
    use crate::vl::events::LoopCommandRequest;

    #[test]
    fn parse_manage_loops_add_request() {
        let request = parse_manage_loops_tool_request(serde_json::json!({
            "action": "add",
            "label": "forge",
            "interval": "5m",
            "prompt": "check forge"
        }))
        .expect("valid request");

        assert_eq!(
            request,
            LoopCommandRequest::Add {
                label: "forge".to_string(),
                interval_seconds: 300,
                prompt_text: "check forge".to_string(),
                goal_text: None,
                auto_remove_on_completion: None,
            }
        );
    }

    #[test]
    fn manage_loops_dynamic_tool_accepts_flat_and_namespaced_aliases() {
        assert!(is_manage_loops_dynamic_tool(None, "manage_loops"));
        assert!(is_manage_loops_dynamic_tool(
            Some("codex_app"),
            "manage_loops"
        ));
        assert!(is_manage_loops_dynamic_tool(
            Some("functions"),
            "manage_loops"
        ));
        assert!(!is_manage_loops_dynamic_tool(
            Some("other_namespace"),
            "manage_loops"
        ));
        assert!(!is_manage_loops_dynamic_tool(None, "other_tool"));
    }

    #[test]
    fn parse_manage_loops_add_request_with_goal_and_cleanup() {
        let request = parse_manage_loops_tool_request(serde_json::json!({
            "action": "add",
            "label": "forge",
            "interval": "5m",
            "prompt": "check forge",
            "goal": "watch package pipeline",
            "auto_remove_on_completion": true
        }))
        .expect("valid request");

        assert_eq!(
            request,
            LoopCommandRequest::Add {
                label: "forge".to_string(),
                interval_seconds: 300,
                prompt_text: "check forge".to_string(),
                goal_text: Some("watch package pipeline".to_string()),
                auto_remove_on_completion: Some(true),
            }
        );
    }

    #[test]
    fn parse_manage_loops_add_request_with_internal_fn_payload() {
        let request = parse_manage_loops_tool_request(serde_json::json!({
            "action": "add",
            "label": "status",
            "interval": "5m",
            "payload": {
                "type": "internal_fn",
                "fn_name": "loop.status",
                "args": {"message": "still watching"}
            }
        }))
        .expect("valid request");

        let LoopCommandRequest::Add { prompt_text, .. } = request else {
            panic!("expected add request");
        };
        let payload = crate::vl::loop_runtime::LoopJobPayload::from_storage_text(&prompt_text);
        assert_eq!(
            payload,
            crate::vl::loop_runtime::LoopJobPayload::InternalFn {
                fn_name: "loop.status".to_string(),
                args: serde_json::json!({"message": "still watching"}),
            }
        );
    }

    #[test]
    fn parse_manage_loops_update_request_supports_partial_updates() {
        let request = parse_manage_loops_tool_request(serde_json::json!({
            "action": "update",
            "label": "forge",
            "goal": null,
            "enabled": false
        }))
        .expect("valid request");

        assert_eq!(
            request,
            LoopCommandRequest::Update {
                label: "forge".to_string(),
                interval_seconds: None,
                prompt_text: None,
                goal_text: Some(None),
                auto_remove_on_completion: None,
                enabled: Some(false),
            }
        );
    }

    #[test]
    fn parse_manage_loops_trigger_request() {
        let request = parse_manage_loops_tool_request(serde_json::json!({
            "action": "trigger",
            "label": "forge"
        }))
        .expect("valid request");

        assert_eq!(
            request,
            LoopCommandRequest::Trigger {
                label: "forge".to_string(),
            }
        );
    }

    #[test]
    fn parse_manage_loops_rejects_short_interval() {
        let error = parse_manage_loops_tool_request(serde_json::json!({
            "action": "add",
            "label": "forge",
            "interval": "5s",
            "prompt": "check forge"
        }))
        .expect_err("interval should be rejected");

        assert!(error.to_string().contains("interval"));
    }
}
