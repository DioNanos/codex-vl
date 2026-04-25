#[path = "vivling/model.rs"]
mod model;
#[path = "vivling/registry.rs"]
mod registry;
#[path = "vivling/runtime.rs"]
mod runtime;
#[path = "vivling/zed.rs"]
mod zed;

pub(crate) use model::VivlingLoopEvent;
pub(crate) use model::VivlingLoopEventKind;
pub(crate) use model::VivlingLoopEventSource;
pub(crate) use runtime::Vivling;
pub(crate) use runtime::VivlingAction;
pub(crate) use runtime::VivlingAssistRequest;
pub(crate) use runtime::VivlingBrainProfileRequest;
pub(crate) use runtime::VivlingBrainProfileRequestKind;
pub(crate) use runtime::VivlingBrainRequestKind;
pub(crate) use runtime::VivlingCommandOutcome;
pub(crate) use runtime::VivlingLoopTickRequest;
pub(crate) use runtime::VivlingLoopTickResult;
pub(crate) use runtime::VivlingPanelData;

#[cfg(test)]
mod tests {
    use super::model::VivlingAiMode;
    use super::model::VivlingState;
    use super::model::VivlingUpgrade;
    use super::registry::VivlingAvailability;
    use super::registry::VivlingRarity;
    use super::registry::active_footer_sprites_for_species;
    use super::registry::active_species_registry;
    use super::registry::hatch_species;
    use super::registry::species_for_id;
    use super::registry::species_registry;
    use super::runtime::VivlingAction;
    use super::zed::ZedTopic;
    use super::zed::zed_panel_data;
    use super::zed::zed_summary_for_topic;

    #[test]
    fn action_parse_treats_unknown_as_direct_message() {
        assert_eq!(
            VivlingAction::parse("feed src/main.rs"),
            Ok(VivlingAction::DirectMessage("feed src/main.rs".to_string()))
        );
    }

    #[test]
    fn action_parse_falls_back_to_direct_message() {
        assert_eq!(
            VivlingAction::parse("ciao come stai"),
            Ok(VivlingAction::DirectMessage("ciao come stai".to_string()))
        );
    }

    #[test]
    fn roster_has_expected_split() {
        let registry = species_registry();
        assert_eq!(registry.len(), 100);
        assert_eq!(
            registry
                .iter()
                .filter(|species| species.rarity == VivlingRarity::Common)
                .count(),
            60
        );
        assert_eq!(
            registry
                .iter()
                .filter(|species| species.rarity == VivlingRarity::Rare)
                .count(),
            30
        );
        assert_eq!(
            registry
                .iter()
                .filter(|species| species.rarity == VivlingRarity::Legendary)
                .count(),
            10
        );
    }

    #[test]
    fn hatch_never_returns_legendary_species() {
        for seed in 0..512 {
            assert_ne!(hatch_species(seed).rarity, VivlingRarity::Legendary);
        }
    }

    #[test]
    fn legendary_species_are_reserved_and_hidden_from_active_roster() {
        assert_eq!(active_species_registry().len(), 90);
        assert!(
            active_species_registry()
                .iter()
                .all(|species| species.availability == VivlingAvailability::Active)
        );
        assert!(
            species_registry()
                .iter()
                .filter(|species| species.rarity == VivlingRarity::Legendary)
                .all(|species| species.availability == VivlingAvailability::Reserved)
        );
    }

    #[test]
    fn every_species_has_three_ascii_forms() {
        for species in species_registry() {
            assert!(
                !species.ascii_baby.is_empty(),
                "missing baby art for {}",
                species.id
            );
            assert!(
                !species.ascii_juvenile.is_empty(),
                "missing juvenile art for {}",
                species.id
            );
            assert!(
                !species.ascii_adult.is_empty(),
                "missing adult art for {}",
                species.id
            );
        }
    }

    #[test]
    fn every_species_has_active_footer_frames() {
        for species in species_registry() {
            let baby_frames = active_footer_sprites_for_species(species, super::model::Stage::Baby);
            let juvenile_frames =
                active_footer_sprites_for_species(species, super::model::Stage::Juvenile);
            let adult_frames =
                active_footer_sprites_for_species(species, super::model::Stage::Adult);
            assert!(baby_frames.iter().all(|frame| !frame.is_empty()));
            assert!(juvenile_frames.iter().all(|frame| !frame.is_empty()));
            assert!(adult_frames.iter().all(|frame| !frame.is_empty()));
        }
    }

    #[test]
    fn common_and_rare_species_have_explicit_stage_card_assets() {
        for species in species_registry()
            .iter()
            .filter(|species| species.rarity != VivlingRarity::Legendary)
        {
            for lines in [
                &species.card_baby.narrow_lines,
                &species.card_baby.wide_lines,
                &species.card_juvenile.narrow_lines,
                &species.card_juvenile.wide_lines,
                &species.card_adult.narrow_lines,
                &species.card_adult.wide_lines,
            ] {
                assert!(
                    !lines.is_empty(),
                    "missing stage card art for {}",
                    species.id
                );
                assert!(
                    lines.iter().any(|line| !line.trim().is_empty()),
                    "blank stage card art for {}",
                    species.id
                );
            }
        }
    }

    #[test]
    fn common_and_rare_species_have_distinct_card_signatures() {
        use std::collections::HashSet;

        let signatures = species_registry()
            .iter()
            .filter(|species| species.rarity != VivlingRarity::Legendary)
            .map(|species| species.card_adult.narrow_lines.join("\n"))
            .collect::<HashSet<_>>();

        assert_eq!(signatures.len(), 90);
    }

    #[test]
    fn growth_requires_active_work_days_to_unlock_level_30() {
        let mut state = VivlingState::default();
        state.work_xp = 10_000;
        state.active_work_days = 29;
        state.recompute_level();
        assert_eq!(state.level, 29);
        assert_eq!(state.stage().label(), "baby");

        state.active_work_days = 30;
        state.recompute_level();
        assert!(state.level >= 30);
        assert_eq!(state.stage().label(), "juvenile");
    }

    #[test]
    fn growth_requires_active_work_days_to_unlock_level_60() {
        let mut state = VivlingState::default();
        state.work_xp = 20_000;
        state.active_work_days = 89;
        state.recompute_level();
        assert_eq!(state.level, 59);
        assert_eq!(state.stage().label(), "juvenile");

        state.active_work_days = 90;
        state.recompute_level();
        assert!(state.level >= 60);
        assert_eq!(state.stage().label(), "adult");
    }

    #[test]
    fn direct_chat_is_available_but_more_mature_after_juvenile() {
        let mut state = VivlingState::default();
        let reply = state
            .direct_chat_reply("ciao")
            .expect("baby reply should work");
        assert!(reply.contains("Tone") || reply.contains("tiny"));

        state.level = 30;
        assert!(state.direct_chat_reply("ciao").is_ok());
    }

    #[test]
    fn active_mode_requires_adult_stage() {
        let mut state = VivlingState::default();
        let err = state
            .set_ai_mode(VivlingAiMode::On)
            .expect_err("adult stage required");
        assert!(err.contains("adult stage"));
    }

    #[test]
    fn active_help_requires_mode_on() {
        let mut state = VivlingState {
            level: 60,
            ai_mode: VivlingAiMode::Off,
            ..Default::default()
        };
        let err = state
            .assist_reply("organize this")
            .expect_err("active mode required");
        assert!(err.contains("mode on"));
    }

    #[test]
    fn work_capsules_drive_affinities() {
        let mut state = VivlingState::default();
        state.record_turn_completed(Some("review the README and audit the change"));
        assert!(state.work_affinities.reviewer > 0);
    }

    #[test]
    fn legacy_species_ids_still_resolve() {
        assert_eq!(species_for_id("bytebud").name, "Bytebud");
        assert_eq!(species_for_id("rootglow").name, "Rootglow");
    }

    #[test]
    fn action_parse_supports_card_and_upgrade() {
        assert_eq!(VivlingAction::parse("card"), Ok(VivlingAction::Card));
        assert_eq!(VivlingAction::parse("upgrade"), Ok(VivlingAction::Upgrade));
    }

    #[test]
    fn work_capsules_set_pending_upgrade_notice() {
        let mut state = VivlingState::default();
        state.work_xp = 10_000;
        state.active_work_days = 29;
        state.recompute_level();

        state.active_work_days = 30;
        let stage = state.recompute_level();
        assert_eq!(stage, Some(super::model::Stage::Juvenile));
        assert_eq!(state.pending_upgrade, Some(VivlingUpgrade::YoungVoice));
    }

    #[test]
    fn zed_panel_young_voice_has_clear_next_step() {
        let panel = zed_panel_data(
            ZedTopic::YoungVoice,
            "ZED: your Vivling reached juvenile stage.",
        );
        assert!(panel.title.contains("Young Voice"));
        assert!(
            panel
                .narrow_lines
                .iter()
                .any(|line| line.contains("`/vivling <message>`"))
        );
    }

    #[test]
    fn zed_panel_active_mode_has_enable_path() {
        let panel = zed_panel_data(
            ZedTopic::ActiveMode,
            "ZED: your Vivling reached adult stage.",
        );
        assert!(panel.title.contains("Active Mode"));
        assert!(
            panel
                .wide_lines
                .iter()
                .any(|line| line.contains("`/vivling mode on`"))
        );
    }

    #[test]
    fn juvenile_suggest_warns_on_loop_churn_without_runtime_success() {
        let mut state = VivlingState {
            level: 30,
            loop_admin_churn: 4,
            loop_runtime_submissions: 0,
            ..Default::default()
        };
        let suggestion = state.suggest();
        assert!(
            suggestion.contains("I see churn") || suggestion.contains("loop unless state changed")
        );
    }

    #[test]
    fn adult_suggest_mentions_busy_loop_friction() {
        let mut state = VivlingState {
            level: 60,
            ai_mode: VivlingAiMode::On,
            loop_blocked_busy: 2,
            ..Default::default()
        };
        let suggestion = state.suggest();
        assert!(suggestion.contains("Busy-turn friction") || suggestion.contains("verify state"));
    }

    #[test]
    fn zed_loop_onboarding_topic_has_loop_next_step() {
        let panel = zed_panel_data(
            ZedTopic::LoopOnboarding,
            &zed_summary_for_topic(ZedTopic::LoopOnboarding),
        );
        assert!(panel.title.contains("Loop Onboarding"));
        assert!(
            panel
                .narrow_lines
                .iter()
                .any(|line| line.contains("one loop") || line.contains("explicit goal"))
        );
    }
}
