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

    fn sample_scene() -> CrtScene<'static> {
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
        }
    }
}
