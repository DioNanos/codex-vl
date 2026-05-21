//! codex-vl: Slash-command boundary for `/vivling`, `/vl`, `/loop`.
//!
//! Keeps the Vivling/Loop dispatch body out of the upstream-heavy
//! `slash_dispatch.rs`. The dispatch entry points stay there as thin
//! match arms that delegate here; this module owns all body logic for
//! the three custom commands, plus the shared outcome renderer.

use super::ChatWidget;
use crate::bottom_pane::VivlingCardView;
use crate::bottom_pane::VivlingUpgradeView;
use crate::vivling::VivlingAction;
use crate::vivling::VivlingBrainRequestKind;
use crate::vivling::VivlingCommandOutcome;
use crate::vl::VivlingLogKind;
use crate::vl::VlEvent;
use crate::vl::events::LoopCommandRequest;

pub(super) const LOOP_USAGE: &str = "Usage: /loop add <label> <interval> <prompt...> | /loop ls | /loop show <label> | /loop on <label> | /loop off <label> | /loop rm <label> | /loop owner [main|vivling]";

const VIVLING_ALIAS_USAGE: &str = "Usage: /vl <message>";

/// `/loop` with no args: show usage hint.
pub(super) fn dispatch_loop_bare(cw: &mut ChatWidget) {
    cw.add_info_message(LOOP_USAGE.to_string(), None);
}

/// `/loop <subcommand> [args...]` — parse + emit `VlEvent::LoopCommand`.
pub(super) fn dispatch_loop_with_args(cw: &mut ChatWidget, trimmed: &str) {
    let Some(thread_id) = cw.thread_id else {
        cw.add_error_message("'/loop' is unavailable before the session starts.".to_string());
        return;
    };
    let Some(request) = parse_loop_command(trimmed) else {
        cw.add_error_message(LOOP_USAGE.to_string());
        return;
    };
    cw.app_event_tx
        .send_vl(VlEvent::LoopCommand { thread_id, request });
}

/// `/vivling [args]` — full Vivling action dispatch.
pub(super) fn dispatch_vivling(cw: &mut ChatWidget, args: &str) {
    cw.sync_vivling_live_context();
    let outcome = VivlingAction::parse(args)
        .and_then(|action| cw.bottom_pane.run_vivling_command(&cw.config, action));
    render_vivling_outcome(cw, outcome);
}

/// `/vl` alias (bare): show usage hint.
pub(super) fn dispatch_vivling_alias_bare(cw: &mut ChatWidget) {
    cw.add_error_message(VIVLING_ALIAS_USAGE.to_string());
}

/// `/vl <message>` alias: status keyword or chat message.
pub(super) fn dispatch_vivling_alias(cw: &mut ChatWidget, args: &str) {
    cw.sync_vivling_live_context();
    let trimmed = args.trim();
    if trimmed.is_empty() {
        cw.add_error_message(VIVLING_ALIAS_USAGE.to_string());
        return;
    }
    let action = if trimmed.eq_ignore_ascii_case("status") {
        VivlingAction::Status
    } else {
        VivlingAction::Chat(trimmed.to_string())
    };
    let outcome = cw.bottom_pane.run_vivling_command(&cw.config, action);
    render_vivling_outcome(cw, outcome);
}

/// Shared renderer for `VivlingCommandOutcome` produced by `/vivling` and `/vl`.
/// Preserves the exact behavior of the previous duplicated arms in
/// `slash_dispatch::{dispatch_vivling_command, dispatch_vivling_direct_alias}`.
fn render_vivling_outcome(cw: &mut ChatWidget, outcome: Result<VivlingCommandOutcome, String>) {
    match outcome {
        Ok(VivlingCommandOutcome::Message(message)) => {
            cw.add_vivling_message(message, VivlingLogKind::Chat);
        }
        Ok(VivlingCommandOutcome::OpenCard(data)) => {
            let view = VivlingCardView::new(data);
            cw.bottom_pane.show_view(Box::new(view));
            cw.request_redraw();
        }
        Ok(VivlingCommandOutcome::OpenUpgrade(data)) => {
            let view = VivlingUpgradeView::new(data);
            cw.bottom_pane.show_view(Box::new(view));
            cw.request_redraw();
        }
        Ok(VivlingCommandOutcome::SpawnNarration { message, panel }) => {
            // codex-vl iter 1C: L1 chat-history message + L2 ZED Lineage
            // panel. Newborn stays inactive — the panel narrates the
            // lineage event without giving the child any operational
            // surface.
            cw.add_vivling_message(message, VivlingLogKind::Chat);
            let view = VivlingUpgradeView::new(panel);
            cw.bottom_pane.show_view(Box::new(view));
            cw.request_redraw();
        }
        Ok(VivlingCommandOutcome::DispatchAssist(request)) => {
            let log_kind = match &request.kind {
                VivlingBrainRequestKind::Chat => VivlingLogKind::Chat,
                VivlingBrainRequestKind::Assist => VivlingLogKind::Assist,
            };
            // Memory V2 Step 12.B.H: keep the "thinking…" line short.
            // `add_vivling_message` already prefixes it with
            // `Vivling: ` (often the Vivling's name elsewhere), so
            // "Vivling brain chat is thinking…" produced
            // "Vivling: Vivling brain chat is thinking…" — a double
            // "Vivling" that read as noise. The pending line now just
            // says "thinking…" with no extra framing.
            let pending_message = "thinking…".to_string();
            cw.app_event_tx
                .send_vl(VlEvent::RunVivlingAssist { request });
            cw.add_vivling_message(pending_message, log_kind);
        }
        Ok(VivlingCommandOutcome::CrtBrainRefresh) => {
            // Memory V2 Step 12.B.H: force a single Expression
            // refresh that bypasses the 60s throttle. Budget /
            // opt-out / dedup still gate the dispatch — if any of
            // those refuses, we surface a hint so the user knows
            // the channel did not actually fire.
            if cw.maybe_trigger_vivling_expression_refresh_forced() {
                cw.add_info_message("CRT brain: refresh dispatched.".to_string(), None);
            } else {
                cw.add_info_message(
                    "CRT brain: refresh skipped (mode off, budget exhausted, or planner had no signal).".to_string(),
                    Some("Run `/vivling crt-brain show` for counters.".to_string()),
                );
            }
        }
        Ok(VivlingCommandOutcome::PersistBrainProfile(request)) => {
            cw.app_event_tx
                .send_vl(VlEvent::PersistVivlingBrainProfile { request });
        }
        Err(message) => cw.add_error_message(message),
    }
}

fn parse_loop_command(args: &str) -> Option<LoopCommandRequest> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut parts = trimmed.split_whitespace();
    let subcommand = parts.next()?;
    match subcommand {
        "ls" => Some(LoopCommandRequest::List),
        "show" => Some(LoopCommandRequest::Show {
            label: parts.next()?.to_string(),
        }),
        "on" => Some(LoopCommandRequest::Enable {
            label: parts.next()?.to_string(),
        }),
        "off" => Some(LoopCommandRequest::Disable {
            label: parts.next()?.to_string(),
        }),
        "rm" => Some(LoopCommandRequest::Remove {
            label: parts.next()?.to_string(),
        }),
        "owner" => match parts.next() {
            None => Some(LoopCommandRequest::OwnerShow),
            Some("main") => Some(LoopCommandRequest::OwnerSetMain),
            Some("vivling") => Some(LoopCommandRequest::OwnerSetVivling),
            Some(_) => None,
        },
        "add" => {
            let label = parts.next()?.to_string();
            let interval_token = parts.next()?;
            let prompt_text = parts.collect::<Vec<_>>().join(" ");
            let interval_seconds = parse_loop_interval_seconds(interval_token)?;
            if prompt_text.trim().is_empty() {
                return None;
            }
            Some(LoopCommandRequest::Add {
                label,
                interval_seconds,
                prompt_text,
                goal_text: None,
                auto_remove_on_completion: None,
            })
        }
        _ => None,
    }
}

fn parse_loop_interval_seconds(token: &str) -> Option<i64> {
    if token.len() < 2 {
        return None;
    }
    let (value, unit) = token.split_at(token.len() - 1);
    let value = value.parse::<i64>().ok()?;
    let interval_seconds = match unit {
        "s" => value,
        "m" => value * 60,
        "h" => value * 3600,
        _ => return None,
    };
    ((30..=86_400).contains(&interval_seconds)).then_some(interval_seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_loop_command_recognizes_subcommands() {
        assert!(matches!(
            parse_loop_command("ls"),
            Some(LoopCommandRequest::List)
        ));
        assert!(matches!(
            parse_loop_command("show x"),
            Some(LoopCommandRequest::Show { label }) if label == "x"
        ));
        assert!(matches!(
            parse_loop_command("on x"),
            Some(LoopCommandRequest::Enable { label }) if label == "x"
        ));
        assert!(matches!(
            parse_loop_command("off x"),
            Some(LoopCommandRequest::Disable { label }) if label == "x"
        ));
        assert!(matches!(
            parse_loop_command("rm x"),
            Some(LoopCommandRequest::Remove { label }) if label == "x"
        ));
        assert!(matches!(
            parse_loop_command("owner"),
            Some(LoopCommandRequest::OwnerShow)
        ));
        assert!(matches!(
            parse_loop_command("owner main"),
            Some(LoopCommandRequest::OwnerSetMain)
        ));
        assert!(matches!(
            parse_loop_command("owner vivling"),
            Some(LoopCommandRequest::OwnerSetVivling)
        ));
        assert!(parse_loop_command("owner xxx").is_none());
        assert!(parse_loop_command("").is_none());
        assert!(parse_loop_command("nope").is_none());
    }

    #[test]
    fn parse_loop_command_add_validates_interval_and_prompt() {
        let req = parse_loop_command("add nightly 30s do a thing").expect("add parses");
        match req {
            LoopCommandRequest::Add {
                label,
                interval_seconds,
                prompt_text,
                goal_text,
                auto_remove_on_completion,
            } => {
                assert_eq!(label, "nightly");
                assert_eq!(interval_seconds, 30);
                assert_eq!(prompt_text, "do a thing");
                assert!(goal_text.is_none());
                assert!(auto_remove_on_completion.is_none());
            }
            other => panic!("expected Add, got {other:?}"),
        }
        // No prompt → reject.
        assert!(parse_loop_command("add nightly 30s").is_none());
        // Empty-only prompt → reject.
        assert!(parse_loop_command("add nightly 30s    ").is_none());
        // Bad interval token → reject.
        assert!(parse_loop_command("add nightly abc do").is_none());
    }

    #[test]
    fn parse_loop_interval_seconds_respects_units_and_bounds() {
        assert_eq!(parse_loop_interval_seconds("30s"), Some(30));
        assert_eq!(parse_loop_interval_seconds("5m"), Some(300));
        assert_eq!(parse_loop_interval_seconds("1h"), Some(3600));
        // Below 30s lower bound.
        assert!(parse_loop_interval_seconds("10s").is_none());
        // Above 24h upper bound.
        assert!(parse_loop_interval_seconds("25h").is_none());
        // Unknown unit.
        assert!(parse_loop_interval_seconds("10x").is_none());
        // Too short.
        assert!(parse_loop_interval_seconds("s").is_none());
    }
}
