mod chronosworn;
mod orchestra;
mod syllo;
mod zed;

use super::super::director::CrtMode;
use crate::vivling::Stage;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SheetState {
    Idle,
    Work,
    Think,
    Happy,
    Sleep,
    Alert,
    Success,
    Error,
}

pub(crate) fn state_for_mode(mode: CrtMode) -> SheetState {
    match mode {
        CrtMode::Idle => SheetState::Idle,
        CrtMode::Thinking => SheetState::Think,
        CrtMode::Working => SheetState::Work,
        CrtMode::Alert => SheetState::Alert,
        CrtMode::Tired => SheetState::Sleep,
        CrtMode::Hungry => SheetState::Idle,
    }
}

pub(crate) fn frame(
    species_id: &str,
    stage: Stage,
    state: SheetState,
) -> Option<&'static [&'static str; 3]> {
    match species_id {
        "syllo" => Some(syllo::frame(stage, state)),
        "orchestra" => Some(orchestra::frame(stage, state)),
        "chronosworn" => Some(chronosworn::frame(stage, state)),
        "zed" => Some(zed::frame(stage, state)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHEET_MAX_WIDTH: usize = 16;
    const SPECIES: &[&str] = &["syllo", "orchestra", "chronosworn", "zed"];
    const STAGES: &[Stage] = &[Stage::Baby, Stage::Juvenile, Stage::Adult];
    const STATES: &[SheetState] = &[
        SheetState::Idle,
        SheetState::Work,
        SheetState::Think,
        SheetState::Happy,
        SheetState::Sleep,
        SheetState::Alert,
        SheetState::Success,
        SheetState::Error,
    ];

    #[test]
    fn every_species_stage_state_combo_has_a_sheet_frame() {
        let mut count = 0;
        for species in SPECIES {
            for stage in STAGES {
                for state in STATES {
                    assert!(frame(species, *stage, *state).is_some());
                    count += 1;
                }
            }
        }
        assert_eq!(count, 96);
        assert!(frame("missing", Stage::Baby, SheetState::Idle).is_none());
    }

    #[test]
    fn sheet_rows_are_ascii_only_and_three_lines() {
        for species in SPECIES {
            for stage in STAGES {
                for state in STATES {
                    let rows = frame(species, *stage, *state).expect("sheet frame");
                    assert_eq!(rows.len(), 3);
                    let width = rows[0].chars().count();
                    assert!((7..=SHEET_MAX_WIDTH).contains(&width));
                    for row in rows {
                        assert_eq!(row.chars().count(), width);
                        assert!(row.is_ascii(), "{species} {stage:?} {state:?}: {row:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn state_for_mode_table_is_stable() {
        assert_eq!(state_for_mode(CrtMode::Idle), SheetState::Idle);
        assert_eq!(state_for_mode(CrtMode::Thinking), SheetState::Think);
        assert_eq!(state_for_mode(CrtMode::Working), SheetState::Work);
        assert_eq!(state_for_mode(CrtMode::Alert), SheetState::Alert);
        assert_eq!(state_for_mode(CrtMode::Tired), SheetState::Sleep);
        assert_eq!(state_for_mode(CrtMode::Hungry), SheetState::Idle);
    }
}
