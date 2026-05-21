mod brain;
mod crt_brain;
mod hatch;
mod import_export;
mod language;
mod lineage;
mod setup;
mod state_ops;
mod status;

use super::*;

impl Vivling {
    pub(crate) fn command(
        &mut self,
        action: VivlingAction,
        cwd: &Path,
    ) -> Result<VivlingCommandOutcome, String> {
        match action {
            VivlingAction::Hatch => self.hatch().map(VivlingCommandOutcome::Message),
            VivlingAction::Dashboard => {
                self.dashboard_message().map(VivlingCommandOutcome::Message)
            }
            VivlingAction::Help => Ok(VivlingCommandOutcome::Message(self.help_message())),
            VivlingAction::Status => self.status().map(VivlingCommandOutcome::Message),
            VivlingAction::Roster => self.roster_summary().map(VivlingCommandOutcome::Message),
            VivlingAction::Focus(target) => self.focus(&target).map(VivlingCommandOutcome::Message),
            VivlingAction::Spawn => self
                .spawn_vivling()
                .map(|(message, panel)| VivlingCommandOutcome::SpawnNarration { message, panel }),
            VivlingAction::Export(path) => self
                .export_active(cwd, path.as_deref())
                .map(VivlingCommandOutcome::Message),
            VivlingAction::Import(path) => self
                .import_package(cwd, &path)
                .map(VivlingCommandOutcome::Message),
            VivlingAction::Remove(target) => self
                .remove_vivling(&target)
                .map(VivlingCommandOutcome::Message),
            VivlingAction::Memory => self
                .update_existing(|state| state.memory_digest())
                .map(VivlingCommandOutcome::Message),
            VivlingAction::Card => self
                .update_existing_value(render_vivling_card)
                .map(VivlingCommandOutcome::OpenCard),
            VivlingAction::Upgrade => self
                .update_existing_value(render_upgrade_card)
                .map(VivlingCommandOutcome::OpenUpgrade),
            VivlingAction::Assist(task) => self
                .prepare_assist_request(&task)
                .map(VivlingCommandOutcome::DispatchAssist),
            VivlingAction::Chat(text) => self.chat(&text),
            VivlingAction::Brain(enabled) => self
                .set_brain_enabled_with_guidance(enabled)
                .map(VivlingCommandOutcome::Message),
            VivlingAction::ModelShow => self.model_summary().map(VivlingCommandOutcome::Message),
            VivlingAction::ModelList => self.model_list().map(VivlingCommandOutcome::Message),
            VivlingAction::ModelProfile(profile) => self
                .prepare_existing_profile_request(profile)
                .map(VivlingCommandOutcome::PersistBrainProfile),
            VivlingAction::ModelCustom {
                model,
                provider,
                effort,
            } => self
                .prepare_custom_profile_request(model, provider, effort)
                .map(VivlingCommandOutcome::PersistBrainProfile),
            VivlingAction::Recap => self
                .update_existing(|state| state.memory_recap())
                .map(VivlingCommandOutcome::Message),
            VivlingAction::PromoteEarly => self
                .update_existing(|state| state.promote_to_level_10_seed())
                .map(VivlingCommandOutcome::Message),
            VivlingAction::PromoteAdult => self
                .update_existing(|state| state.promote_to_adult_seed())
                .map(VivlingCommandOutcome::Message),
            VivlingAction::Mode(mode) => self
                .update_existing_result(|state| state.set_ai_mode(mode))
                .map(VivlingCommandOutcome::Message),
            VivlingAction::DirectMessage(text) => self
                .update_existing_result(|state| state.direct_chat_reply(&text))
                .map(VivlingCommandOutcome::Message),
            VivlingAction::Reset => {
                self.state = None;
                let removed_id = self.active_vivling_id.take();
                if let Some(path) = self.active_state_path() {
                    let _ = fs::remove_file(path);
                }
                if let Some(id) = removed_id {
                    let _ = self.remove_from_roster(&id);
                }
                Ok(VivlingCommandOutcome::Message(
                    "Vivling reset. Use /vivling hatch when you want a new one.".to_string(),
                ))
            }
            VivlingAction::Zed => self
                .open_zed_companion()
                .map(VivlingCommandOutcome::OpenUpgrade),
            VivlingAction::CrtBrain(crt_brain_action) => match crt_brain_action {
                super::action::CrtBrainAction::Show => {
                    self.crt_brain_show().map(VivlingCommandOutcome::Message)
                }
                super::action::CrtBrainAction::On => self
                    .crt_brain_set(codex_vivling_core::model::VivlingExpressionMode::On)
                    .map(VivlingCommandOutcome::Message),
                super::action::CrtBrainAction::Off => self
                    .crt_brain_set(codex_vivling_core::model::VivlingExpressionMode::Off)
                    .map(VivlingCommandOutcome::Message),
                super::action::CrtBrainAction::Default => self
                    .crt_brain_set(codex_vivling_core::model::VivlingExpressionMode::Default)
                    .map(VivlingCommandOutcome::Message),
            },
            VivlingAction::Language(language_action) => match language_action {
                super::action::LanguageAction::Show => self
                    .show_language_status()
                    .map(VivlingCommandOutcome::Message),
                super::action::LanguageAction::Auto => self
                    .set_language_override(None)
                    .map(VivlingCommandOutcome::Message),
                super::action::LanguageAction::Set(code) => self
                    .set_language_override(Some(code))
                    .map(VivlingCommandOutcome::Message),
                super::action::LanguageAction::Mode(mode_str) => self
                    .set_language_mode(&mode_str)
                    .map(VivlingCommandOutcome::Message),
            },
        }
    }
}
