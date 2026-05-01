mod model;
#[path = "vivling/registry.rs"]
mod registry;
mod runtime;
#[path = "vivling/zed.rs"]
mod zed;

pub(crate) use model::Stage;
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
pub(crate) use runtime::VivlingLiveContext;
pub(crate) use runtime::VivlingLiveStatusItem;
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
        assert_eq!(registry.len(), 16);
        assert_eq!(
            registry
                .iter()
                .filter(|species| species.rarity == VivlingRarity::Common)
                .count(),
            6
        );
        assert_eq!(
            registry
                .iter()
                .filter(|species| species.rarity == VivlingRarity::Rare)
                .count(),
            5
        );
        assert_eq!(
            registry
                .iter()
                .filter(|species| species.rarity == VivlingRarity::Legendary)
                .count(),
            4
        );
        assert_eq!(
            registry
                .iter()
                .filter(|species| species.rarity == VivlingRarity::Mythic)
                .count(),
            1
        );
    }

    #[test]
    fn hatch_returns_only_active_common_or_rare_species() {
        for seed in 0..512 {
            assert_ne!(hatch_species(seed).rarity, VivlingRarity::Legendary);
            assert_ne!(hatch_species(seed).rarity, VivlingRarity::Mythic);
        }
    }

    #[test]
    fn legendary_and_mythic_species_are_reserved_and_hidden_from_active_roster() {
        let active = active_species_registry();
        let active_ids: Vec<&str> = active.iter().map(|species| species.id.as_str()).collect();
        // 0.126.0 phase pilot: only Syllo and Orchestra appear in the
        // active hatch pool. Other Common/Rare species stay in the registry
        // for legacy save resolution but are filtered out of the roster.
        assert_eq!(active_ids, vec!["syllo", "orchestra"]);
        assert!(
            active
                .iter()
                .all(|species| species.availability == VivlingAvailability::Active)
        );
        assert!(
            species_registry()
                .iter()
                .filter(|species| {
                    matches!(
                        species.rarity,
                        VivlingRarity::Legendary | VivlingRarity::Mythic
                    )
                })
                .all(|species| species.availability == VivlingAvailability::Reserved)
        );
    }

    #[test]
    fn fresh_state_starts_as_syllo_with_only_syllo_unlocked() {
        let state = VivlingState::new(super::model::SeedIdentity {
            value: "fresh-test-seed".to_string(),
            install_id: Some("install-test".to_string()),
        });
        assert_eq!(state.species, "syllo");
        assert_eq!(state.unlocked_species, vec!["syllo".to_string()]);
    }

    #[test]
    fn hatch_with_only_syllo_unlocked_returns_syllo_for_many_hashes() {
        let unlocked = vec!["syllo".to_string()];
        for hash in 0u64..256 {
            let species = super::registry::hatch_species_from_unlocked(hash, &unlocked);
            assert_eq!(species.id, "syllo", "hash {hash} should hatch syllo");
        }
    }

    #[test]
    fn hatch_with_orchestra_unlocked_can_pick_either() {
        let unlocked = vec!["syllo".to_string(), "orchestra".to_string()];
        let mut saw_syllo = false;
        let mut saw_orchestra = false;
        for hash in 0u64..64 {
            match super::registry::hatch_species_from_unlocked(hash, &unlocked)
                .id
                .as_str()
            {
                "syllo" => saw_syllo = true,
                "orchestra" => saw_orchestra = true,
                other => panic!("unexpected hatch result: {other}"),
            }
        }
        assert!(saw_syllo);
        assert!(saw_orchestra);
    }

    #[test]
    fn hatch_with_empty_unlock_set_falls_back_to_syllo() {
        let unlocked: Vec<String> = Vec::new();
        let species = super::registry::hatch_species_from_unlocked(42, &unlocked);
        assert_eq!(species.id, "syllo");
    }

    #[test]
    fn syllo_adult_grants_orchestra_exactly_once() {
        let mut state = VivlingState {
            species: "syllo".to_string(),
            unlocked_species: VivlingState::default_unlocked_species(),
            ..Default::default()
        };
        let granted = state.apply_stage_unlocks(super::model::Stage::Adult);
        assert_eq!(granted, vec!["orchestra".to_string()]);
        assert!(state.unlocked_species.iter().any(|id| id == "orchestra"));
        // Idempotent — second call must not duplicate.
        let granted_again = state.apply_stage_unlocks(super::model::Stage::Adult);
        assert!(granted_again.is_empty());
        let count = state
            .unlocked_species
            .iter()
            .filter(|id| id.as_str() == "orchestra")
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn orchestra_adult_grants_chronosworn_exactly_once() {
        let mut state = VivlingState {
            species: "orchestra".to_string(),
            unlocked_species: vec!["syllo".to_string(), "orchestra".to_string()],
            ..Default::default()
        };
        let granted = state.apply_stage_unlocks(super::model::Stage::Adult);
        assert_eq!(granted, vec!["chronosworn".to_string()]);
        assert!(state.unlocked_species.iter().any(|id| id == "chronosworn"));
        let granted_again = state.apply_stage_unlocks(super::model::Stage::Adult);
        assert!(granted_again.is_empty());
    }

    #[test]
    fn stage_transitions_never_grant_zed() {
        for species_id in ["syllo", "orchestra", "chronosworn"] {
            let mut state = VivlingState {
                species: species_id.to_string(),
                unlocked_species: VivlingState::default_unlocked_species(),
                ..Default::default()
            };
            for stage in [
                super::model::Stage::Baby,
                super::model::Stage::Juvenile,
                super::model::Stage::Adult,
            ] {
                state.apply_stage_unlocks(stage);
            }
            assert!(
                !state.unlocked_species.iter().any(|id| id == "zed"),
                "{species_id} stage chain should never auto-grant ZED"
            );
        }
    }

    #[test]
    fn non_adult_stages_grant_nothing() {
        let mut state = VivlingState {
            species: "syllo".to_string(),
            unlocked_species: VivlingState::default_unlocked_species(),
            ..Default::default()
        };
        assert!(
            state
                .apply_stage_unlocks(super::model::Stage::Baby)
                .is_empty()
        );
        assert!(
            state
                .apply_stage_unlocks(super::model::Stage::Juvenile)
                .is_empty()
        );
        assert_eq!(state.unlocked_species, vec!["syllo".to_string()]);
    }

    #[test]
    fn legacy_state_without_unlock_field_normalizes_to_syllo_unlocked() {
        // Legacy save: serialized JSON missing `unlocked_species`. Serde
        // `#[serde(default)]` produces an empty Vec; `normalize_loaded_state`
        // (and its `normalize_unlocked_species` step) must restore Syllo.
        let legacy_json = r#"{
            "version": 1,
            "hatched": true,
            "visible": true,
            "seed_hash": "deadbeef",
            "vivling_id": "viv-legacy",
            "install_id": "install-legacy",
            "species": "syllo",
            "rarity": "Common",
            "name": "Legacy",
            "primary_vivling_id": "viv-legacy",
            "is_primary": true,
            "level": 1
        }"#;
        let mut state: VivlingState = serde_json::from_str(legacy_json)
            .expect("legacy state must deserialize with serde defaults");
        assert!(state.unlocked_species.is_empty());
        state.normalize_loaded_state();
        assert_eq!(state.unlocked_species, vec!["syllo".to_string()]);
    }

    #[test]
    fn normalize_unlocked_species_dedups_and_drops_empty() {
        let mut state = VivlingState {
            unlocked_species: vec![
                "orchestra".to_string(),
                "".to_string(),
                "  ".to_string(),
                "orchestra".to_string(),
                "syllo".to_string(),
            ],
            ..Default::default()
        };
        state.normalize_loaded_state();
        let mut sorted = state.unlocked_species.clone();
        sorted.sort();
        assert_eq!(sorted, vec!["orchestra".to_string(), "syllo".to_string()]);
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
            .filter(|species| species.availability == VivlingAvailability::Active)
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
            .filter(|species| species.availability == VivlingAvailability::Active)
            .map(|species| species.card_adult.narrow_lines.join("\n"))
            .collect::<HashSet<_>>();

        assert_eq!(signatures.len(), 11);
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
    fn legacy_species_ids_fall_back_to_new_roster() {
        assert_eq!(species_for_id("bytebud").name, "Syllo");
        assert_eq!(species_for_id("rootglow").name, "Syllo");
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
