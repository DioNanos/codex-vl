//! One-line Vivling face strip — **vl extension**, NOT from the canon.
//!
//! The canon `vivling_crt_face_state_sprites_2026-05-15.md` specifies 3-line
//! sprites and contains no one-line variant. Only the state-marker legend is
//! taken from it: idle `o`, work `<`, think `?`, alert `!`, tired `-`,
//! hungry `o`, error `x`, always inside the face, never appended on the side.
//!
//! This module defines the one-line strip as a vl extension, following the
//! canon's *principles*: width-exact frames (always 5 terminal columns, no jitter),
//! stable silhouette, and motion on a single accent character — the same way
//! the 3-line bodies animate (`.-o-.` → `.-o-'`). Frame B differs from frame A in
//! **exactly one** outline char (right edge `.` → `'`); the marker never moves.
//! Error, Alert and Tired faces use their canonical state markers; Error is static.

use super::director::CrtMode;

/// Display width of every strip frame, in terminal columns.
#[allow(dead_code)]
pub(crate) const STRIP_WIDTH: u16 = 5;

const IDLE: &[&str] = &[".-o-.", ".-o-'"];
const THINK: &[&str] = &[".-?-.", ".-?-'"];
const WORK: &[&str] = &[".-<-.", ".-<-'"];
const ALERT: &[&str] = &[".-!-."];
const TIRED: &[&str] = &[".---."];
const HUNGRY: &[&str] = &[".-o-.", ".-o-'"];
const ERROR: &[&str] = &[".-x-."];

fn cycle(mode: CrtMode) -> &'static [&'static str] {
    match mode {
        CrtMode::Idle => IDLE,
        CrtMode::Thinking => THINK,
        CrtMode::Working => WORK,
        CrtMode::Alert => ALERT,
        CrtMode::Tired => TIRED,
        CrtMode::Hungry => HUNGRY,
        CrtMode::Error => ERROR,
    }
}

/// Returns the one-line face for `mode` at `tick` (cycles within the state).
pub(crate) fn face(mode: CrtMode, tick: u64) -> &'static str {
    let frames = cycle(mode);
    frames[(tick as usize) % frames.len()]
}

/// The canon state-marker legend: idle `o`, work `<`, think `?`, alert `!`,
/// tired `-`, hungry `o`, error `x`. Always the SAME marker embedded in the face.
#[allow(dead_code)]
pub(crate) fn marker(mode: CrtMode) -> char {
    match mode {
        CrtMode::Idle => 'o',
        CrtMode::Thinking => '?',
        CrtMode::Working => '<',
        CrtMode::Alert => '!',
        CrtMode::Tired => '-',
        CrtMode::Hungry => 'o',
        CrtMode::Error => 'x',
    }
}

/// Derive the approved 5-column face from an existing sprite's top row.
///
/// Strips any surrounding whitespace padding without blindly copying prefix bytes (which
/// might be whitespace padding or stray delimiters), identifies the canonical
/// state marker (`!`, `-`, `x`, `?`, `<`, `o`, `^`), and returns the exact 5-column
/// canonical face preserving the internal marker.
#[allow(dead_code)]
pub(crate) fn face_from_top_row(top_row: &str) -> Option<&'static str> {
    let trimmed = top_row.trim();
    // Direct match against known faces or adult/juvenile top rows
    match trimmed {
        ".-o-." | ".-o-'" | "\\.o./" | ".o." | "o-.-o" | "((>))" | "/O\\" | "\\ O /" => {
            return Some(".-o-.");
        }
        ".-?-." | ".-?-'" | "\\.?./" | ".?." | "((>))?" | "/O\\?" | "\\ O /?" => {
            return Some(".-?-.");
        }
        ".-<-." | ".-<-'" | "\\.<./" | ".<." | ".-< >-." | "--.-< >-." => {
            return Some(".-<-.");
        }
        ".-!-." | "\\.!./" | ".!." | "o-!-o" | "((>))!" | "/O\\!" | "\\ O /!" | "!.-< >-.!" => {
            return Some(".-!-.");
        }
        ".---." | "\\.../" | "..." | "o---o" | "((>))z" | "/O\\z" | "\\ O /z" => {
            return Some(".---.");
        }
        ".-x-." | "\\.x./" | ".x." | "o-x-o" | "((>))x" | "/O\\x" | "\\ O /x" => {
            return Some(".-x-.");
        }
        ".-^-." | "\\.^./" | ".^." => return Some(".-^-."),
        _ => {}
    }
    // Inspect markers within the face
    if trimmed.contains('!') {
        Some(".-!-.")
    } else if trimmed.contains('x') {
        Some(".-x-.")
    } else if trimmed.contains('?') {
        Some(".-?-.")
    } else if trimmed.contains('<') {
        Some(".-<-.")
    } else if trimmed.contains('z')
        || (trimmed.contains('-')
            && (trimmed.starts_with('.') || trimmed.contains("---") || trimmed.starts_with('o')))
        || trimmed.contains("...")
    {
        Some(".---.")
    } else if trimmed.contains('o') || trimmed.contains('O') || trimmed.contains("(>))") {
        Some(".-o-.")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_frame_is_five_columns() {
        for mode in [
            CrtMode::Idle,
            CrtMode::Thinking,
            CrtMode::Working,
            CrtMode::Alert,
            CrtMode::Tired,
            CrtMode::Hungry,
            CrtMode::Error,
        ] {
            for f in cycle(mode) {
                assert_eq!(
                    f.chars().count() as u16,
                    STRIP_WIDTH,
                    "frame {f:?} not width {STRIP_WIDTH} for {mode:?}"
                );
                assert_eq!(
                    crate::width::display_width(f) as u16,
                    STRIP_WIDTH,
                    "frame {f:?} not display width {STRIP_WIDTH} for {mode:?}"
                );
            }
        }
    }

    #[test]
    fn markers_match_canon_legend() {
        assert!(face(CrtMode::Idle, 0).contains('o'));
        assert!(face(CrtMode::Working, 0).contains('<'));
        assert!(face(CrtMode::Thinking, 0).contains('?'));
        assert!(face(CrtMode::Alert, 0).contains('!'));
        assert!(face(CrtMode::Tired, 0).contains('-'));
        assert!(face(CrtMode::Hungry, 0).contains('o'));
        assert!(face(CrtMode::Error, 0).contains('x'));
    }

    #[test]
    fn marker_matches_the_same_canon_legend_as_face() {
        for mode in [
            CrtMode::Idle,
            CrtMode::Thinking,
            CrtMode::Working,
            CrtMode::Alert,
            CrtMode::Tired,
            CrtMode::Hungry,
            CrtMode::Error,
        ] {
            assert!(
                face(mode, 0).contains(marker(mode)),
                "{mode:?}: marker {:?} not found in face {:?}",
                marker(mode),
                face(mode, 0)
            );
        }
        assert_eq!(marker(CrtMode::Idle), 'o');
        assert_eq!(marker(CrtMode::Working), '<');
        assert_eq!(marker(CrtMode::Thinking), '?');
        assert_eq!(marker(CrtMode::Alert), '!');
        assert_eq!(marker(CrtMode::Tired), '-');
        assert_eq!(marker(CrtMode::Hungry), 'o');
        assert_eq!(marker(CrtMode::Error), 'x');
    }

    #[test]
    fn frame_b_differs_in_exactly_one_char() {
        for mode in [CrtMode::Idle, CrtMode::Thinking, CrtMode::Working] {
            let a = face(mode, 0);
            let b = face(mode, 1);
            assert_ne!(a, b, "{mode:?} must animate");
            let diff = a.chars().zip(b.chars()).filter(|(x, y)| x != y).count();
            assert_eq!(diff, 1, "{mode:?}: frame B must differ in exactly 1 char");
            // The marker (center char) never moves.
            assert_eq!(a.chars().nth(2), b.chars().nth(2));
        }
    }

    #[test]
    fn error_and_alert_and_tired_are_static() {
        assert_eq!(face(CrtMode::Error, 0), face(CrtMode::Error, 999));
        assert_eq!(face(CrtMode::Alert, 0), face(CrtMode::Alert, 999));
        assert_eq!(face(CrtMode::Tired, 0), face(CrtMode::Tired, 999));
    }

    #[test]
    fn wraps_on_tick_and_never_panics() {
        assert_eq!(face(CrtMode::Idle, 0), face(CrtMode::Idle, 2));
        let _ = face(CrtMode::Idle, u64::MAX);
    }

    #[test]
    fn face_from_top_row_derives_correct_face_for_all_species_and_stages() {
        use crate::vivling::Stage;
        use crate::vl::crt::sprites::art_for;
        use crate::vl::crt::tier::CrtTier;

        for species in ["syllo", "orchestra", "chronosworn", "zed"] {
            for stage in [Stage::Baby, Stage::Juvenile, Stage::Adult] {
                for (mode, expected_marker) in [
                    (CrtMode::Alert, '!'),
                    (CrtMode::Tired, '-'),
                    (CrtMode::Error, 'x'),
                    (CrtMode::Hungry, 'o'),
                ] {
                    let frame = art_for(species, stage, mode, CrtTier::Rich, 0);
                    let top_row = frame.rows[0];
                    let derived = face_from_top_row(top_row);
                    assert!(
                        derived.is_some(),
                        "failed to derive face from {species} {stage:?} {mode:?}: {top_row:?}"
                    );
                    let f = derived.unwrap();
                    assert_eq!(f.chars().count(), 5);
                    assert!(
                        f.contains(expected_marker),
                        "derived face {f:?} does not contain expected marker '{expected_marker}' for {species} {stage:?} {mode:?}"
                    );
                }
            }
        }
    }
}
