//! Dispatch for codex-vl app events (`AppEvent::Vl(VlEvent)`).
//!
//! Keeping the custom event arms in a dedicated impl block limits the
//! surface of our changes to `event_dispatch.rs`, so upstream edits to
//! the main dispatcher do not have to be merged around our code.

use color_eyre::eyre::Result;

use super::App;
use super::AppRunControl;
use crate::legacy_core::config::ConfigBuilder;
use crate::legacy_core::config::ConfigOverrides;
use crate::legacy_core::config::LoaderOverrides;
use crate::legacy_core::config::edit::ConfigEdit;
use crate::legacy_core::config::edit::ConfigEditsBuilder;
use crate::vl::VlEvent;

impl App {
    pub(super) async fn handle_vl_event(&mut self, event: VlEvent) -> Result<AppRunControl> {
        match event {
            VlEvent::LoopCommand { thread_id, request } => {
                self.apply_loop_command_request(
                    thread_id, request, /*source*/ false, /*emit_ui_feedback*/ true,
                )
                .await?;
            }
            VlEvent::ReloadLoopJobs { thread_id } => {
                self.handle_reload_loop_jobs(thread_id).await?;
            }
            VlEvent::LoopTick { thread_id, job_id } => {
                self.handle_loop_tick(thread_id, job_id).await?;
            }
            VlEvent::PersistVivlingBrainProfile { request } => {
                use crate::vivling::VivlingBrainProfileRequestKind;

                let (profile_name, model_to_show) = match &request.kind {
                    VivlingBrainProfileRequestKind::AssignExisting { profile } => {
                        // Upstream rust-v0.134.0-alpha.3 moved per-session profile
                        // selection from ConfigOverrides.config_profile to
                        // LoaderOverrides.user_config_profile (typed ProfileV2Name).
                        let profile_v2 = match profile.parse() {
                            Ok(name) => name,
                            Err(err) => {
                                self.chat_widget.add_error_message(format!(
                                    "Invalid Vivling profile name `{profile}`: {err}"
                                ));
                                return Ok(AppRunControl::Continue);
                            }
                        };
                        let mut loader_overrides = LoaderOverrides::default();
                        loader_overrides.user_config_profile = Some(profile_v2);
                        let resolved = ConfigBuilder::default()
                            .codex_home(self.config.codex_home.to_path_buf())
                            .harness_overrides(ConfigOverrides {
                                cwd: Some(self.config.cwd.to_path_buf()),
                                ..ConfigOverrides::default()
                            })
                            .loader_overrides(loader_overrides)
                            .build()
                            .await;
                        match resolved {
                            Ok(profile_config) => match profile_config.model.clone() {
                                Some(model) => (profile.clone(), model),
                                None => {
                                    self.chat_widget.add_error_message(format!(
                                        "Vivling profile `{profile}` does not resolve to a model."
                                    ));
                                    return Ok(AppRunControl::Continue);
                                }
                            },
                            Err(err) => {
                                self.chat_widget.add_error_message(format!(
                                    "Failed to load Vivling profile `{profile}`: {err}"
                                ));
                                return Ok(AppRunControl::Continue);
                            }
                        }
                    }
                    VivlingBrainProfileRequestKind::CreateOrUpdate {
                        profile,
                        model,
                        provider,
                        effort,
                    } => {
                        let mut edits = vec![ConfigEdit::SetPath {
                            segments: vec![
                                "profiles".to_string(),
                                profile.clone(),
                                "model".to_string(),
                            ],
                            value: toml_edit::value(model.clone()),
                        }];
                        if let Some(provider) = provider {
                            edits.push(ConfigEdit::SetPath {
                                segments: vec![
                                    "profiles".to_string(),
                                    profile.clone(),
                                    "model_provider".to_string(),
                                ],
                                value: toml_edit::value(provider.clone()),
                            });
                        }
                        if let Some(effort) = effort {
                            edits.push(ConfigEdit::SetPath {
                                segments: vec![
                                    "profiles".to_string(),
                                    profile.clone(),
                                    "model_reasoning_effort".to_string(),
                                ],
                                value: toml_edit::value(effort.to_string()),
                            });
                        }
                        match ConfigEditsBuilder::new(&self.config.codex_home)
                            .with_edits(edits)
                            .apply()
                            .await
                        {
                            Ok(()) => (profile.clone(), model.clone()),
                            Err(err) => {
                                self.chat_widget.add_error_message(format!(
                                    "Failed to save Vivling profile `{profile}`: {err}"
                                ));
                                return Ok(AppRunControl::Continue);
                            }
                        }
                    }
                };

                match self
                    .chat_widget
                    .assign_vivling_brain_profile(profile_name.clone())
                {
                    Ok(message) => self.chat_widget.add_info_message(
                        format!("{message} Resolved model `{model_to_show}`."),
                        /*hint*/ None,
                    ),
                    Err(err) => self.chat_widget.add_error_message(err),
                }
            }
            VlEvent::RunVivlingAssist { request } => {
                self.run_vivling_assist(request);
            }
            VlEvent::VivlingAssistFinished {
                vivling_id,
                kind,
                result,
            } => match result {
                Ok(reply) => {
                    if let Err(err) = self.chat_widget.mark_vivling_brain_reply(&reply) {
                        tracing::warn!(
                            "failed to persist Vivling brain reply for {vivling_id}: {err}"
                        );
                        // do not early-exit: bond success bonus must still record
                        // (Codex design review iter 4 §7).
                    }
                    if let Err(err) = self.chat_widget.record_vivling_brain_success(kind) {
                        tracing::warn!(
                            "failed to record Vivling bond success for {vivling_id}: {err}"
                        );
                    }
                    let log_kind = match kind {
                        crate::vivling::VivlingBrainRequestKind::Chat => {
                            crate::vl::VivlingLogKind::Chat
                        }
                        crate::vivling::VivlingBrainRequestKind::Assist => {
                            crate::vl::VivlingLogKind::Assist
                        }
                    };
                    let visible_reply = format_vivling_brain_reply(kind, &reply);
                    self.chat_widget
                        .add_vivling_message(visible_reply, log_kind);
                    // Memory V2 Step 12.B.H: pre-warm the CRT live
                    // phrase after every successful brain reply. Slash
                    // commands like `/vl` do NOT fire the upstream
                    // `record_vivling_turn_completed` hook (that one
                    // only runs at the end of a Codex agent turn), so
                    // without this trigger the CRT footer stays on
                    // the template chain even though the Vivling just
                    // produced fresh content. The Expression channel
                    // still obeys throttle/dedup/budget, so a chat
                    // turn never overspends.
                    self.chat_widget.maybe_trigger_vivling_expression_refresh();
                }
                Err(err) => {
                    if let Err(persist_err) =
                        self.chat_widget.mark_vivling_brain_runtime_error(&err)
                    {
                        tracing::warn!(
                            "failed to persist Vivling brain error for {vivling_id}: {persist_err}"
                        );
                    }
                    self.chat_widget.add_error_message(err);
                }
            },
            VlEvent::RunVivlingLoopTick {
                thread_id,
                job_id,
                request,
            } => {
                self.run_vivling_loop_tick(thread_id, job_id, request);
            }
            VlEvent::VivlingLoopTickFinished {
                thread_id,
                job_id,
                result,
            } => {
                self.handle_vivling_loop_tick_finished(thread_id, job_id, result)
                    .await?;
            }
            VlEvent::RunVivlingExpression { request } => {
                self.run_vivling_expression(request);
            }
            VlEvent::VivlingExpressionFinished { vivling_id, result } => {
                self.handle_vivling_expression_finished(vivling_id, result);
            }
            VlEvent::SidebarPushMessage {
                kind,
                text,
                vivling_id,
            } => {
                self.chat_widget
                    .push_vl_sidebar_message(kind, text, vivling_id);
            }
        }
        Ok(AppRunControl::Continue)
    }

    /// Memory V2 Step 12.B.D.2 — apply or log an Expression LLM
    /// reply. Ok: hand to the chat widget so the runtime CRT /
    /// proactive caches refresh. Err: bump the persisted failure
    /// counter and debug-log; intentionally does NOT touch
    /// `brain_last_error` (the Expression channel is best-effort
    /// background — failures must not pollute `/vl chat` /
    /// `/vivling assist` error surfaces).
    fn handle_vivling_expression_finished(
        &mut self,
        vivling_id: String,
        result: Result<crate::vivling::VivlingExpressionResult, String>,
    ) {
        match result {
            Ok(reply) => {
                let now = chrono::Utc::now();
                if let Err(err) =
                    self.chat_widget
                        .record_vivling_expression_result_for(&vivling_id, &reply, now)
                {
                    tracing::debug!(
                        target: "vivling::expression",
                        "failed to apply expression reply for {vivling_id}: {err}"
                    );
                }
            }
            Err(err) => {
                tracing::debug!(
                    target: "vivling::expression",
                    "Vivling {vivling_id} expression dispatch failed: {err}"
                );
                if let Err(persist_err) = self
                    .chat_widget
                    .record_vivling_expression_failure_for(&vivling_id)
                {
                    tracing::debug!(
                        target: "vivling::expression",
                        "failed to persist expression failure for {vivling_id}: {persist_err}"
                    );
                }
            }
        }
    }
}

fn format_vivling_brain_reply(
    _kind: crate::vivling::VivlingBrainRequestKind,
    reply: &str,
) -> String {
    // Memory V2 Step 12.B.H: drop the `Brain response: ` /
    // `Brain assist: ` prefix. `add_vivling_message` already
    // displays the line under a `Vivling: ` header, so the legacy
    // prefix produced "Vivling: Brain response: Io sono Nilo …" —
    // a double frame the user read as noise. Returning the raw
    // reply lets the Vivling voice speak directly.
    reply.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vivling::VivlingBrainRequestKind;

    #[test]
    fn brain_reply_returns_raw_reply_without_prefix() {
        // Memory V2 Step 12.B.H: legacy "Brain response:" /
        // "Brain assist:" prefix removed. The Vivling voice speaks
        // through `add_vivling_message`'s `Vivling: ` header — no
        // extra framing.
        assert_eq!(
            format_vivling_brain_reply(VivlingBrainRequestKind::Chat, "ready"),
            "ready"
        );
        assert_eq!(
            format_vivling_brain_reply(VivlingBrainRequestKind::Assist, "check logs"),
            "check logs"
        );
    }

    #[test]
    fn brain_reply_preserves_multiline_reply() {
        assert_eq!(
            format_vivling_brain_reply(VivlingBrainRequestKind::Chat, "first\nsecond"),
            "first\nsecond"
        );
    }
}
