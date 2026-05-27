use super::scene::CrtScene;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CrtMode {
    Idle,
    Thinking,
    Working,
    Alert,
    Tired,
    Hungry,
}

pub(crate) struct CrtDirector;

impl CrtDirector {
    pub(crate) fn select(scene: &CrtScene<'_>, width: u16) -> CrtMode {
        match scene.activity {
            Some(crate::vl::VivlingActivity::Eating) => return CrtMode::Hungry,
            Some(crate::vl::VivlingActivity::Sleeping) => return CrtMode::Tired,
            Some(crate::vl::VivlingActivity::Playing) => return CrtMode::Thinking,
            Some(crate::vl::VivlingActivity::Working) => return CrtMode::Working,
            Some(crate::vl::VivlingActivity::Idle) | None => {}
        }
        // codex-vl Step 14 Bug 2 fix — the TUI distinguishes three "busy"
        // concepts (`BottomPane::is_task_running`, `VivlingActivity::Working`,
        // `loop_tick_running`). When the broader TUI is processing a task
        // but the Vivling lifecycle hasn't switched to `Working` yet, the
        // pre-existing low-energy branch below would otherwise win and
        // surface `CrtMode::Alert` (Syllo BABY_ALERT sprite + reversed
        // yellow palette = bright yellow bands) even though the agent is
        // legitimately busy. The render-only hint `tui_task_running`
        // overrides Alert with Working for visual mode only; lifecycle
        // stats and persistence are untouched. Audit: codex-vl-bug-audit:
        // NEEDS_CHANGES:bug-2 (Codex GPT-5.5 2026-05-27).
        if scene.tui_task_running {
            return CrtMode::Working;
        }
        if scene.energy <= 12 {
            return CrtMode::Alert;
        }
        if scene.hunger >= 90 {
            return CrtMode::Hungry;
        }
        if scene.energy <= 28 {
            return CrtMode::Tired;
        }
        if width >= 24 && scene.mood.eq_ignore_ascii_case("curious") {
            return CrtMode::Thinking;
        }
        CrtMode::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn director_turns_stats_into_emotional_scene_modes() {
        let mut scene = sample_scene();
        scene.energy = 9;
        assert_eq!(CrtDirector::select(&scene, 40), CrtMode::Alert);

        scene.energy = 80;
        scene.hunger = 95;
        assert_eq!(CrtDirector::select(&scene, 40), CrtMode::Hungry);

        scene.hunger = 40;
        scene.loop_count = 2;
        assert_eq!(CrtDirector::select(&scene, 40), CrtMode::Thinking);

        scene.activity = Some(crate::vl::VivlingActivity::Working);
        assert_eq!(CrtDirector::select(&scene, 40), CrtMode::Working);
    }

    #[test]
    fn tui_task_running_overrides_low_energy_alert() {
        // codex-vl Step 14 Bug 2 regression guard. Low-energy + idle activity
        // → CrtMode::Alert. Same low energy + tui_task_running override →
        // CrtMode::Working. Verifies the override fires before the low-energy
        // branch so the yellow-band Syllo ALERT sprite does not surface
        // during legitimate TUI busy states.
        let mut scene = sample_scene();
        scene.energy = 9;
        scene.activity = None;
        assert_eq!(CrtDirector::select(&scene, 40), CrtMode::Alert);

        scene.tui_task_running = true;
        assert_eq!(CrtDirector::select(&scene, 40), CrtMode::Working);

        // Without the override, full activity-driven Working still wins.
        scene.tui_task_running = false;
        scene.activity = Some(crate::vl::VivlingActivity::Working);
        assert_eq!(CrtDirector::select(&scene, 40), CrtMode::Working);
    }

    #[test]
    fn tui_task_running_does_not_override_explicit_activity() {
        // Explicit lifecycle activities (Eating/Sleeping/Playing) win over
        // the tui_task_running hint so the user still sees the activity
        // sprite when they have intentionally chosen one.
        let mut scene = sample_scene();
        scene.tui_task_running = true;

        scene.activity = Some(crate::vl::VivlingActivity::Eating);
        assert_eq!(CrtDirector::select(&scene, 40), CrtMode::Hungry);

        scene.activity = Some(crate::vl::VivlingActivity::Sleeping);
        assert_eq!(CrtDirector::select(&scene, 40), CrtMode::Tired);

        scene.activity = Some(crate::vl::VivlingActivity::Playing);
        assert_eq!(CrtDirector::select(&scene, 40), CrtMode::Thinking);
    }

    fn sample_scene() -> CrtScene<'static> {
        use super::super::animation::TransitionPhases;
        use super::super::animation::VivlingCrtConfig;
        // Test-only: leak a default config so the borrow has 'static lifetime.
        // Cheap and self-contained; only used in this unit test.
        let cfg: &'static VivlingCrtConfig = Box::leak(Box::new(VivlingCrtConfig::default()));
        CrtScene {
            species_id: "syllo",
            stage: crate::vivling::Stage::Baby,
            name: "Nilo",
            level: 5,
            role: "builder",
            mood: "curious",
            energy: 73,
            hunger: 74,
            loop_count: 0,
            sprite: "('.')=  .",
            seed: 7,
            elapsed_ms: 240,
            last_message: None,
            activity: None,
            tier: super::super::tier::CrtTier::Safe,
            crt_config: cfg,
            transitions: TransitionPhases {
                mode_fade: 1.0,
                message_reveal_chars: usize::MAX,
                insight_slide: 1.0,
            },
            tui_task_running: false,
        }
    }
}
