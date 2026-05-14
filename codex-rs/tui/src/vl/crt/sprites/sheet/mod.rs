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
) -> Option<&'static [[&'static str; 3]]> {
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
    fn every_species_stage_state_combo_has_a_sheet_cycle() {
        let mut count = 0;
        for species in SPECIES {
            for stage in STAGES {
                for state in STATES {
                    let frames = frame(species, *stage, *state).expect("sheet cycle");
                    assert!(
                        !frames.is_empty(),
                        "{species} {stage:?} {state:?} has empty cycle"
                    );
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
                    let frames = frame(species, *stage, *state).expect("sheet cycle");
                    for rows in frames {
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
    }

    /// Codex iter 7 §1 vincolo: all frames in a single cycle must have the
    /// IDENTICAL width (max row width). `compose_at` recenters on width
    /// change and would make the Vivling jitter on tick advance.
    #[test]
    fn frame_cycle_exact_width_invariant() {
        for species in SPECIES {
            for stage in STAGES {
                for state in STATES {
                    let frames = frame(species, *stage, *state).expect("sheet cycle");
                    let widths: Vec<usize> =
                        frames.iter().map(|rows| rows[0].chars().count()).collect();
                    if let Some(first) = widths.first() {
                        for w in &widths {
                            assert_eq!(
                                w, first,
                                "{species} {stage:?} {state:?} cycle width drift: {widths:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// Syllo (active pool 0.126.0) has 2-frame animated cycles for the
    /// principal states (Idle / Work / Think / Happy) on every stage.
    #[test]
    fn syllo_idle_alternates_with_tick() {
        for stage in STAGES {
            let frames = syllo::frame(*stage, SheetState::Idle);
            assert_eq!(
                frames.len(),
                2,
                "syllo {stage:?} Idle expected 2 frames, got {}",
                frames.len()
            );
            assert_ne!(
                frames[0], frames[1],
                "syllo {stage:?} Idle frames must differ"
            );
        }
    }

    /// Orchestra (active pool 0.126.0) also animates principal states.
    #[test]
    fn orchestra_idle_alternates_with_tick() {
        for stage in STAGES {
            let frames = orchestra::frame(*stage, SheetState::Idle);
            assert_eq!(
                frames.len(),
                2,
                "orchestra {stage:?} Idle expected 2 frames, got {}",
                frames.len()
            );
            assert_ne!(
                frames[0], frames[1],
                "orchestra {stage:?} Idle frames must differ"
            );
        }
    }

    /// Chronosworn + ZED remain single-frame in this iter (reserved species).
    /// Regression guard: any future change to multi-frame must be intentional.
    #[test]
    fn chronosworn_zed_single_frame_invariant() {
        for stage in STAGES {
            for state in STATES {
                assert_eq!(
                    chronosworn::frame(*stage, *state).len(),
                    1,
                    "chronosworn {stage:?} {state:?} expected single frame"
                );
                assert_eq!(
                    zed::frame(*stage, *state).len(),
                    1,
                    "zed {stage:?} {state:?} expected single frame"
                );
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
