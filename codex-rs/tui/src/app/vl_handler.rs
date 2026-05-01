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
                        let resolved = ConfigBuilder::default()
                            .codex_home(self.config.codex_home.to_path_buf())
                            .harness_overrides(ConfigOverrides {
                                cwd: Some(self.config.cwd.to_path_buf()),
                                config_profile: Some(profile.clone()),
                                ..ConfigOverrides::default()
                            })
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
            VlEvent::VivlingAssistFinished { vivling_id, result } => match result {
                Ok(reply) => {
                    if let Err(err) = self.chat_widget.mark_vivling_brain_reply(&reply) {
                        tracing::warn!(
                            "failed to persist Vivling brain reply for {vivling_id}: {err}"
                        );
                    }
                    self.chat_widget
                        .add_vivling_message(reply, crate::vl::VivlingLogKind::Assist);
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
}
