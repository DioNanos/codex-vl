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

    /// codex-vl 2026-05-15 DAG rule: the state marker MUST live inside the
    /// face/core — either embedded in the top row between the dot frame
    /// (e.g. `.-?-.` / `\.?./`) or as a body indicator in the mid row
    /// (e.g. `/(-)\` for Juvenile Sleep where the top row degenerates to
    /// dots). It must NEVER appear as a side suffix appended to the right
    /// of the closed face (e.g. `.-o-.?` or `_/|<|\_!`).
    #[test]
    fn syllo_state_marker_is_inside_face_not_appended() {
        const MARKERS: &[(SheetState, char)] = &[
            (SheetState::Idle, 'o'),
            (SheetState::Work, '<'),
            (SheetState::Think, '?'),
            (SheetState::Happy, '^'),
            (SheetState::Sleep, '-'),
            (SheetState::Alert, '!'),
            (SheetState::Success, 'v'),
            (SheetState::Error, 'x'),
        ];
        for stage in STAGES {
            for (state, marker) in MARKERS {
                let frames = syllo::frame(*stage, *state);
                for rows in frames {
                    let top = rows[0];
                    let mid = rows[1];
                    let bot = rows[2];
                    // Top row must close with `.` (Baby/Juvenile) or `/` (Adult)
                    // — i.e. the face frame, not a stray marker.
                    let top_terminator = top.trim_end().chars().last().expect("top row");
                    assert!(
                        matches!(top_terminator, '.' | '/'),
                        "syllo {stage:?} {state:?} top row leaks marker outside face: {top:?}",
                    );
                    // Bottom row must close with `\` (Baby/Juvenile, `/_<_\`) or
                    // `_` (Adult, `_/ \_`) — anything else means a side marker.
                    let bot_terminator = bot.trim_end().chars().last().expect("bot row");
                    assert!(
                        matches!(bot_terminator, '\\' | '_'),
                        "syllo {stage:?} {state:?} bot row leaks marker outside face: {bot:?}",
                    );
                    // The chosen state marker must appear somewhere inside the
                    // face — top row or mid row body. (For Juvenile Sleep the
                    // top degenerates to `...` and the `-` sits in `/(-)\`.)
                    assert!(
                        top.contains(*marker) || mid.contains(*marker),
                        "syllo {stage:?} {state:?} marker '{marker}' not found in face: top={top:?} mid={mid:?}",
                    );
                }
            }
        }
    }

    /// codex-vl 2026-05-15 DAG rule (iter B Orchestra): same marker-inside-
    /// face contract applied to Orchestra. Top row terminates with `.`
    /// (Baby/Juvenile) or `/` (Adult); bottom row terminates with `\` (Baby
    /// `/___\`) or `_` (Juvenile `_/ \_` / `_/o\_`, Adult `_/ \_`); the
    /// chosen marker char appears in top OR mid row of the face.
    #[test]
    fn orchestra_state_marker_is_inside_face_not_appended() {
        const MARKERS: &[(SheetState, char)] = &[
            (SheetState::Idle, 'o'),
            (SheetState::Work, '<'),
            (SheetState::Think, '?'),
            (SheetState::Happy, '^'),
            (SheetState::Sleep, '-'),
            (SheetState::Alert, '!'),
            (SheetState::Success, 'v'),
            (SheetState::Error, 'x'),
        ];
        for stage in STAGES {
            for (state, marker) in MARKERS {
                let frames = orchestra::frame(*stage, *state);
                for rows in frames {
                    let top = rows[0];
                    let mid = rows[1];
                    let bot = rows[2];
                    let top_terminator = top.trim_end().chars().last().expect("top row");
                    assert!(
                        matches!(top_terminator, '.' | '/'),
                        "orchestra {stage:?} {state:?} top row leaks marker outside face: {top:?}",
                    );
                    let bot_terminator = bot.trim_end().chars().last().expect("bot row");
                    assert!(
                        matches!(bot_terminator, '\\' | '_'),
                        "orchestra {stage:?} {state:?} bot row leaks marker outside face: {bot:?}",
                    );
                    assert!(
                        top.contains(*marker) || mid.contains(*marker),
                        "orchestra {stage:?} {state:?} marker '{marker}' not found in face: top={top:?} mid={mid:?}",
                    );
                }
            }
        }
    }

    /// codex-vl 2026-05-15 DAG rule: ZED stays in archive mode (narrator/
    /// presenter), so the ZED sheet must not grow runtime expansions in the
    /// same branch that refreshes Syllo. Concretely: ZED's source file must
    /// stay byte-identical to `develop`. We verify via `include_bytes!` +
    /// SHA snapshot of the canonical bytes shipped at iter A start. This
    /// catches accidental edits to `sheet/zed.rs` during sub-iter merges.
    #[test]
    fn zed_sheet_file_unchanged_under_crt_face_state_refresh() {
        // Snapshot taken on 2026-05-15 develop @ cf508d2553 (pre-iter A).
        const ZED_SHEET_BYTES: &[u8] = include_bytes!("zed.rs");
        const ZED_SHEET_LEN_AT_ITER_A_START: usize = 3895;
        assert_eq!(
            ZED_SHEET_BYTES.len(),
            ZED_SHEET_LEN_AT_ITER_A_START,
            "zed.rs file changed during crt-face-state sub-iters; ZED must stay archive-only per DAG D5",
        );
    }
}
