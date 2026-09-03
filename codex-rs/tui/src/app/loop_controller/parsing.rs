//! codex-vl loop_controller: input parsers.
//!
//! Pure parsing utilities: command JSON / interval tokens / Vivling
//! status strings / `manage_loops` dynamic tool requests.

use crate::vl::events::LoopCommandRequest;
use crate::vl::loop_runtime::LoopJobPayload;
use codex_state::LoopRunnerKind;

use super::formatting::LOOP_STATUS_BLOCKED;
use super::formatting::LOOP_STATUS_DONE;
use super::formatting::LOOP_STATUS_PROGRESS;

pub(super) const MANAGE_LOOPS_TOOL_NAMESPACE: &str = "codex_app";
pub(super) const MANAGE_LOOPS_TOOL_NAME: &str = "manage_loops";

pub(in crate::app) fn is_manage_loops_dynamic_tool(namespace: Option<&str>, tool: &str) -> bool {
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
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    strategy: Option<String>,
    #[serde(default)]
    runner: Option<String>,
    #[serde(default)]
    runner_model: Option<String>,
    #[serde(default)]
    schedule_kind: Option<String>,
    #[serde(default)]
    schedule_at: Option<String>,
    #[serde(default)]
    one_shot: Option<String>,
    #[serde(default)]
    tz: Option<String>,
    #[serde(default)]
    rearm_on_boot: Option<bool>,
}

fn parse_runner_kind(raw: Option<String>) -> anyhow::Result<LoopRunnerKind> {
    match raw
        .as_deref()
        .unwrap_or("main")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "main" => Ok(LoopRunnerKind::Main),
        "child_agent" => Ok(LoopRunnerKind::ChildAgent),
        other => Err(anyhow::anyhow!(
            "`runner` must be `main` or `child_agent`, got `{other}`"
        )),
    }
}

fn parse_runner_model(raw: Option<String>) -> anyhow::Result<Option<String>> {
    raw.map(|model| {
        let model = model.trim().to_string();
        if model.is_empty() {
            Err(anyhow::anyhow!(
                "`runner_model` cannot be empty when provided"
            ))
        } else {
            Ok(model)
        }
    })
    .transpose()
}

/// RFC 3339 with a mandatory offset: a naive datetime
/// is rejected as `one_shot_requires_offset`, never silently assumed UTC.
/// Returns epoch-ms UTC.
fn parse_one_shot_ms(raw: Option<&str>) -> anyhow::Result<Option<i64>> {
    let Some(value) = raw else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    match chrono::DateTime::parse_from_rfc3339(value) {
        Ok(parsed) => Ok(Some(parsed.timestamp_millis())),
        Err(_) => {
            let naive_parse = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
                .or_else(|_| chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S"));
            if naive_parse.is_ok() {
                Err(anyhow::anyhow!(
                    "one_shot_requires_offset: pass an RFC 3339 timestamp with an explicit offset, e.g. 2026-10-25T02:30:00+02:00"
                ))
            } else {
                Err(anyhow::anyhow!(
                    "`one_shot` must be an RFC 3339 timestamp with an explicit offset, e.g. 2026-10-25T02:30:00+02:00"
                ))
            }
        }
    }
}

/// schedule fields for `add` (default `interval`, validated as a
/// triplet): returns (schedule_kind, schedule_at, one_shot_at_ms, tz).
fn parse_schedule_fields(
    schedule_kind: Option<&str>,
    schedule_at: Option<&str>,
    one_shot: Option<&str>,
    tz: Option<&str>,
) -> anyhow::Result<(String, Option<String>, Option<i64>, Option<String>)> {
    let kind = match schedule_kind.map(str::trim).filter(|v| !v.is_empty()) {
        Some(kind) => kind.to_string(),
        None => "interval".to_string(),
    };
    let schedule_at = schedule_at
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let tz = tz
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let one_shot_at_ms = parse_one_shot_ms(one_shot)?;
    match kind.as_str() {
        "interval" => {
            if schedule_at.is_some() || one_shot_at_ms.is_some() {
                return Err(anyhow::anyhow!(
                    "`schedule_at`/`one_shot`/`tz` apply only to schedule_kind=at|one_shot"
                ));
            }
        }
        "at" => {
            if schedule_at.is_none() {
                return Err(anyhow::anyhow!(
                    "`schedule_at` (HH:MM) is required for schedule_kind=at"
                ));
            }
            if tz.is_none() {
                return Err(anyhow::anyhow!(
                    "`tz` (IANA name, e.g. Europe/Rome) is required for schedule_kind=at"
                ));
            }
            if one_shot_at_ms.is_some() {
                return Err(anyhow::anyhow!(
                    "`one_shot` applies only to schedule_kind=one_shot"
                ));
            }
        }
        "one_shot" => {
            if one_shot_at_ms.is_none() {
                return Err(anyhow::anyhow!(
                    "`one_shot` (RFC 3339 with offset) is required for schedule_kind=one_shot"
                ));
            }
            if schedule_at.is_some() {
                return Err(anyhow::anyhow!(
                    "`schedule_at` applies only to schedule_kind=at"
                ));
            }
        }
        other => {
            return Err(anyhow::anyhow!(
                "`schedule_kind` must be interval, at, or one_shot (got `{other}`)"
            ));
        }
    }
    Ok((kind, schedule_at, one_shot_at_ms, tz))
}

/// schedule fields for `update`: `None` leaves the schedule untouched;
/// a non-`None` schedule revalidates the whole triplet in the same call.
type UpdateSchedule = Option<(String, Option<String>, Option<i64>, Option<String>)>;

fn parse_schedule_fields_update(
    schedule_kind: Option<&str>,
    schedule_at: Option<&str>,
    one_shot: Option<&str>,
    tz: Option<&str>,
) -> anyhow::Result<UpdateSchedule> {
    let touched = [schedule_kind, schedule_at, one_shot, tz]
        .iter()
        .any(|value| value.is_some());
    if !touched {
        return Ok(None);
    }
    let (kind, schedule_at, one_shot_at_ms, tz) =
        parse_schedule_fields(schedule_kind, schedule_at, one_shot, tz)?;
    Ok(Some((kind, schedule_at, one_shot_at_ms, tz)))
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
        "delegate" => Ok(LoopCommandRequest::Delegate {
            label: args
                .label
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`label` is required for delegate"))?,
            owner_kind: args
                .owner
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`owner` is required for delegate"))?,
        }),
        "undelegate" => Ok(LoopCommandRequest::Undelegate {
            label: args
                .label
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`label` is required for undelegate"))?,
        }),
        "delegation" => Ok(LoopCommandRequest::Delegation { label: args.label }),
        "strategy" => Ok(LoopCommandRequest::SetStrategy {
            label: args
                .label
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`label` is required for strategy"))?,
            strategy: args
                .strategy
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`strategy` is required for strategy"))?,
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
            let runner_kind = parse_runner_kind(args.runner)?;
            let runner_model = parse_runner_model(args.runner_model)?;
            let (schedule_kind, schedule_at, one_shot_at_ms, tz) = parse_schedule_fields(
                args.schedule_kind.as_deref(),
                args.schedule_at.as_deref(),
                args.one_shot.as_deref(),
                args.tz.as_deref(),
            )?;
            Ok(LoopCommandRequest::Add {
                label,
                interval_seconds,
                prompt_text,
                goal_text: parse_add_goal(goal_argument)?,
                auto_remove_on_completion: args.auto_remove_on_completion,
                runner_kind,
                runner_model,
                schedule_kind,
                schedule_at,
                one_shot_at_ms,
                tz,
                rearm_on_boot: args.rearm_on_boot,
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
            let runner_kind = args
                .runner
                .map(|runner| parse_runner_kind(Some(runner)))
                .transpose()?;
            let runner_model = args
                .runner_model
                .map(|model| parse_runner_model(Some(model)))
                .transpose()?;
            let schedule = parse_schedule_fields_update(
                args.schedule_kind.as_deref(),
                args.schedule_at.as_deref(),
                args.one_shot.as_deref(),
                args.tz.as_deref(),
            )?;
            let (schedule_kind, schedule_at, one_shot_at_ms, tz) = match schedule {
                Some(schedule) => (Some(schedule.0), schedule.1, schedule.2, schedule.3),
                None => (None, None, None, None),
            };
            Ok(LoopCommandRequest::Update {
                label,
                interval_seconds,
                prompt_text,
                goal_text: parse_update_goal(goal_argument)?,
                auto_remove_on_completion: args.auto_remove_on_completion,
                enabled: args.enabled,
                runner_kind,
                runner_model,
                schedule_kind,
                schedule_at,
                one_shot_at_ms,
                tz,
                rearm_on_boot: args.rearm_on_boot,
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
    use codex_state::LoopRunnerKind;

    #[test]
    fn one_shot_without_offset_is_rejected_explicitly() {
        let err = parse_manage_loops_tool_request(serde_json::json!({
            "action": "add",
            "label": "one-shot",
            "interval": "5m",
            "prompt": "fire once",
            "schedule_kind": "one_shot",
            "one_shot": "2026-10-25T02:30:00"
        }))
        .expect_err("naive one_shot must be rejected");

        assert!(err.to_string().contains("one_shot_requires_offset"));
    }

    #[test]
    fn one_shot_with_offset_converts_to_utc_ms() {
        let request = parse_manage_loops_tool_request(serde_json::json!({
            "action": "add",
            "label": "one-shot",
            "interval": "5m",
            "prompt": "fire once",
            "schedule_kind": "one_shot",
            "one_shot": "2026-10-25T02:30:00+02:00"
        }))
        .expect("valid one_shot request");

        let LoopCommandRequest::Add {
            one_shot_at_ms,
            schedule_kind,
            ..
        } = request
        else {
            panic!("expected an add request");
        };
        // 2026-10-25T02:30:00+02:00 == 2026-10-25T00:30:00Z (misurato).
        assert_eq!(one_shot_at_ms, Some(1_792_888_200_000));
        assert_eq!(schedule_kind, "one_shot");
    }

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
                runner_kind: LoopRunnerKind::Main,
                runner_model: None,
                schedule_kind: "interval".to_string(),
                schedule_at: None,
                one_shot_at_ms: None,
                tz: None,
                rearm_on_boot: None,
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
                runner_kind: LoopRunnerKind::Main,
                runner_model: None,
                schedule_kind: "interval".to_string(),
                schedule_at: None,
                one_shot_at_ms: None,
                tz: None,
                rearm_on_boot: None,
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
    fn parse_manage_loops_child_runner_requires_explicit_model_shape() {
        let request = parse_manage_loops_tool_request(serde_json::json!({
            "action": "add",
            "label": "child",
            "interval": "1m",
            "prompt": "check status",
            "runner": "child_agent",
            "runner_model": "gpt-5.3-codex"
        }))
        .expect("valid child runner request");

        assert_eq!(
            request,
            LoopCommandRequest::Add {
                label: "child".to_string(),
                interval_seconds: 60,
                prompt_text: "check status".to_string(),
                goal_text: None,
                auto_remove_on_completion: None,
                runner_kind: LoopRunnerKind::ChildAgent,
                runner_model: Some("gpt-5.3-codex".to_string()),
                schedule_kind: "interval".to_string(),
                schedule_at: None,
                one_shot_at_ms: None,
                tz: None,
                rearm_on_boot: None,
            }
        );

        let error = parse_manage_loops_tool_request(serde_json::json!({
            "action": "add",
            "label": "bad",
            "interval": "1m",
            "prompt": "check status",
            "runner": "worker"
        }))
        .expect_err("closed runner enum must reject unknown values");
        assert!(error.to_string().contains("`child_agent`"));

        let update = parse_manage_loops_tool_request(serde_json::json!({
            "action": "update",
            "label": "child",
            "runner": "child_agent",
            "runner_model": "gpt-5.3-codex"
        }))
        .expect("valid child runner update");
        assert_eq!(
            update,
            LoopCommandRequest::Update {
                label: "child".to_string(),
                interval_seconds: None,
                prompt_text: None,
                goal_text: None,
                auto_remove_on_completion: None,
                enabled: None,
                runner_kind: Some(LoopRunnerKind::ChildAgent),
                runner_model: Some(Some("gpt-5.3-codex".to_string())),
                schedule_kind: None,
                schedule_at: None,
                one_shot_at_ms: None,
                tz: None,
                rearm_on_boot: None,
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
                runner_kind: None,
                runner_model: None,
                schedule_kind: None,
                schedule_at: None,
                one_shot_at_ms: None,
                tz: None,
                rearm_on_boot: None,
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
    fn parse_manage_loops_delegation_requests() {
        assert_eq!(
            parse_manage_loops_tool_request(serde_json::json!({
                "action": "delegate",
                "label": "forge",
                "owner": "vivling"
            }))
            .expect("delegate parses"),
            LoopCommandRequest::Delegate {
                label: "forge".to_string(),
                owner_kind: "vivling".to_string(),
            }
        );
        assert_eq!(
            parse_manage_loops_tool_request(serde_json::json!({
                "action": "strategy",
                "label": "forge",
                "strategy": "suggest"
            }))
            .expect("strategy parses"),
            LoopCommandRequest::SetStrategy {
                label: "forge".to_string(),
                strategy: "suggest".to_string(),
            }
        );
    }

    #[test]
    fn parse_manage_loops_rearm_on_boot_add_and_partial_update() {
        let request = parse_manage_loops_tool_request(serde_json::json!({
            "action": "add",
            "label": "nightly",
            "interval": "5m",
            "prompt": "check nightly",
            "rearm_on_boot": true
        }))
        .expect("valid add with rearm_on_boot");

        assert_eq!(
            request,
            LoopCommandRequest::Add {
                label: "nightly".to_string(),
                interval_seconds: 300,
                prompt_text: "check nightly".to_string(),
                goal_text: None,
                auto_remove_on_completion: None,
                runner_kind: LoopRunnerKind::Main,
                runner_model: None,
                schedule_kind: "interval".to_string(),
                schedule_at: None,
                one_shot_at_ms: None,
                tz: None,
                rearm_on_boot: Some(true),
            }
        );

        let update = parse_manage_loops_tool_request(serde_json::json!({
            "action": "update",
            "label": "nightly",
            "rearm_on_boot": false
        }))
        .expect("valid update with rearm_on_boot");

        assert_eq!(
            update,
            LoopCommandRequest::Update {
                label: "nightly".to_string(),
                interval_seconds: None,
                prompt_text: None,
                goal_text: None,
                auto_remove_on_completion: None,
                enabled: None,
                runner_kind: None,
                runner_model: None,
                schedule_kind: None,
                schedule_at: None,
                one_shot_at_ms: None,
                tz: None,
                rearm_on_boot: Some(false),
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
