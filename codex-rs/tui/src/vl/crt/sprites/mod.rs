mod fallback;
mod sheet;
mod species;

use super::director::CrtMode;
use super::frame::Frame;
use super::tier::CrtTier;
use crate::vivling::Stage;

pub(crate) fn art_for(
    species_id: &str,
    stage: Stage,
    mode: CrtMode,
    tier: CrtTier,
    tick: u64,
) -> Frame {
    match tier {
        CrtTier::Safe => fallback::art_for(mode, tick),
        CrtTier::Rich | CrtTier::Image => species::art_for(species_id, stage, mode, tick)
            .unwrap_or_else(|| fallback::art_for(mode, tick)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(frame: Frame) -> String {
        frame.rows.join("\n")
    }

    #[test]
    fn safe_keeps_fallback_shape() {
        let rendered = joined(art_for(
            "syllo",
            Stage::Baby,
            CrtMode::Working,
            CrtTier::Safe,
            0,
        ));
        assert!(rendered.contains("( >_> )"));
    }

    #[test]
    fn rich_dispatches_by_species() {
        let syllo = joined(art_for(
            "syllo",
            Stage::Baby,
            CrtMode::Idle,
            CrtTier::Rich,
            0,
        ));
        let orchestra = joined(art_for(
            "orchestra",
            Stage::Baby,
            CrtMode::Idle,
            CrtTier::Rich,
            0,
        ));
        assert_ne!(syllo, orchestra);
        assert!(syllo.contains(".-o-."));
        assert!(orchestra.contains("-( o )-"));
    }

    #[test]
    fn rich_dispatches_by_stage() {
        let baby = joined(art_for(
            "syllo",
            Stage::Baby,
            CrtMode::Idle,
            CrtTier::Rich,
            0,
        ));
        let adult = joined(art_for(
            "syllo",
            Stage::Adult,
            CrtMode::Idle,
            CrtTier::Rich,
            0,
        ));
        assert_ne!(baby, adult);
    }

    #[test]
    fn rich_dispatches_modes_through_sheet() {
        let idle = joined(art_for(
            "zed",
            Stage::Juvenile,
            CrtMode::Idle,
            CrtTier::Rich,
            0,
        ));
        let working = joined(art_for(
            "zed",
            Stage::Juvenile,
            CrtMode::Working,
            CrtTier::Rich,
            0,
        ));
        let thinking = joined(art_for(
            "zed",
            Stage::Juvenile,
            CrtMode::Thinking,
            CrtTier::Rich,
            0,
        ));
        let alert = joined(art_for(
            "zed",
            Stage::Juvenile,
            CrtMode::Alert,
            CrtTier::Rich,
            0,
        ));
        let tired = joined(art_for(
            "zed",
            Stage::Juvenile,
            CrtMode::Tired,
            CrtTier::Rich,
            0,
        ));
        let hungry = joined(art_for(
            "zed",
            Stage::Juvenile,
            CrtMode::Hungry,
            CrtTier::Rich,
            0,
        ));

        assert_ne!(idle, working);
        assert_ne!(idle, thinking);
        assert_ne!(idle, alert);
        assert_ne!(idle, tired);
        assert_eq!(hungry, idle);
    }

    #[test]
    fn unknown_species_falls_back() {
        let unknown = joined(art_for(
            "missing",
            Stage::Adult,
            CrtMode::Alert,
            CrtTier::Rich,
            0,
        ));
        let fallback = joined(art_for(
            "syllo",
            Stage::Adult,
            CrtMode::Alert,
            CrtTier::Safe,
            0,
        ));
        assert_eq!(unknown, fallback);
    }
}
