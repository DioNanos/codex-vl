use super::SheetState;
use crate::vivling::Stage;

// codex-vl 2026-05-15: state marker lives INSIDE the face/core, never
// appended on the side (DAG rule, see
// `docs/assets/vivling_crt_face_state_sprites_2026-05-15.md`).
// Animated cycles keep 2 frames (Codex iter 7 §1 exact-width invariant);
// Sleep/Alert/Success/Error are single-frame static states.

// ---------- BABY ----------
// Layout: top `.-X-.` (5) + leading/trailing 2-space pad → 9 cols.
// Mid    `/(YYY)\`        (7) + 1-space pad → 9 cols.
// Bot    `/_<_\`          (5) + 2-space pad → 9 cols.
// Animated F2 just blinks the eyes (`/(   )\` → `/(. .)\`) keeping body
// shape stable for the CRT.

const BABY_IDLE: &[[&str; 3]] = &[
    ["  .-o-.  ", " /(   )\\ ", "  /_<_\\  "],
    ["  .-o-.  ", " /(. .)\\ ", "  /_<_\\  "],
];

const BABY_WORK: &[[&str; 3]] = &[
    ["  .-<-.  ", " /(   )\\ ", "  /_<_\\  "],
    ["  .-<-.  ", " /(. .)\\ ", "  /_<_\\  "],
];

const BABY_THINK: &[[&str; 3]] = &[
    ["  .-?-.  ", " /(   )\\ ", "  /_<_\\  "],
    ["  .-?-.  ", " /(. .)\\ ", "  /_<_\\  "],
];

const BABY_HAPPY: &[[&str; 3]] = &[
    ["  .-^-.  ", " /(^ ^)\\ ", "  /_<_\\  "],
    ["  .-^-.  ", " /(^.^)\\ ", "  /_<_\\  "],
];

const BABY_SLEEP: &[[&str; 3]] = &[["  .---.  ", " /(- -)\\ ", "  /_<_\\  "]];

const BABY_ALERT: &[[&str; 3]] = &[["  .-!-.  ", " /( o )\\ ", "  /_<_\\  "]];

const BABY_SUCCESS: &[[&str; 3]] = &[["  .-v-.  ", " /(^o^)\\ ", "  /_<_\\  "]];

const BABY_ERROR: &[[&str; 3]] = &[["  .-x-.  ", " /( x )\\ ", "  /_<_\\  "]];

// ---------- JUVENILE ----------
// Layout: top `.X.` (3) + 3-space pad → 9 cols.
// Mid    `/(Y)\`  (5) + 2-space pad → 9 cols.
// Bot    `_/|<|\_` (7) + 1-space pad → 9 cols.
// F2 blinks the eye glyph (`/(_)\` → `/(.)\`).

const JUVENILE_IDLE: &[[&str; 3]] = &[
    ["   .o.   ", "  /(_)\\  ", " _/|<|\\_ "],
    ["   .o.   ", "  /(.)\\  ", " _/|<|\\_ "],
];

const JUVENILE_WORK: &[[&str; 3]] = &[
    ["   .<.   ", "  /(_)\\  ", " _/|<|\\_ "],
    ["   .<.   ", "  /(.)\\  ", " _/|<|\\_ "],
];

const JUVENILE_THINK: &[[&str; 3]] = &[
    ["   .?.   ", "  /(_)\\  ", " _/|<|\\_ "],
    ["   .?.   ", "  /(.)\\  ", " _/|<|\\_ "],
];

const JUVENILE_HAPPY: &[[&str; 3]] = &[
    ["   .^.   ", "  /(^)\\  ", " _/|<|\\_ "],
    ["   .^.   ", "  /(*)\\  ", " _/|<|\\_ "],
];

const JUVENILE_SLEEP: &[[&str; 3]] = &[["   ...   ", "  /(-)\\  ", " _/|<|\\_ "]];

const JUVENILE_ALERT: &[[&str; 3]] = &[["   .!.   ", "  /(o)\\  ", " _/|<|\\_ "]];

const JUVENILE_SUCCESS: &[[&str; 3]] = &[["   .v.   ", "  /(^)\\  ", " _/|<|\\_ "]];

const JUVENILE_ERROR: &[[&str; 3]] = &[["   .x.   ", "  /(x)\\  ", " _/|<|\\_ "]];

// ---------- ADULT ----------
// Layout: top `\.X./` (5) + 2-space pad → 9 cols.
// Mid    `--/|\--` (7) + 1-space pad → 9 cols.
// Bot    `_/ \_`   (5) + 2-space pad → 9 cols.
// F2 subtly shifts mid row pad chars (`--` ↔ alternate) for breathing
// without disturbing the central body silhouette.

const ADULT_IDLE: &[[&str; 3]] = &[
    ["  \\.o./  ", " --/|\\-- ", "  _/ \\_  "],
    ["  \\.o./  ", " ==/|\\== ", "  _/ \\_  "],
];

const ADULT_WORK: &[[&str; 3]] = &[
    ["  \\.<./  ", " --/|\\-- ", "  _/ \\_  "],
    ["  \\.<./  ", " ==/|\\== ", "  _/ \\_  "],
];

const ADULT_THINK: &[[&str; 3]] = &[
    ["  \\.?./  ", " --/|\\-- ", "  _/ \\_  "],
    ["  \\.?./  ", " ~~/|\\~~ ", "  _/ \\_  "],
];

const ADULT_HAPPY: &[[&str; 3]] = &[
    ["  \\.^./  ", " --/|\\-- ", "  _/ \\_  "],
    ["  \\.^./  ", " ++/|\\++ ", "  _/ \\_  "],
];

const ADULT_SLEEP: &[[&str; 3]] = &[["  \\.../  ", " --/|\\-- ", "  _/ \\_  "]];

const ADULT_ALERT: &[[&str; 3]] = &[["  \\.!./  ", " --/|\\-- ", "  _/ \\_  "]];

const ADULT_SUCCESS: &[[&str; 3]] = &[["  \\.v./  ", " --/|\\-- ", "  _/ \\_  "]];

const ADULT_ERROR: &[[&str; 3]] = &[["  \\.x./  ", " --/|\\-- ", "  _/ \\_  "]];

pub(super) fn frame(stage: Stage, state: SheetState) -> &'static [[&'static str; 3]] {
    match (stage, state) {
        (Stage::Baby, SheetState::Idle) => BABY_IDLE,
        (Stage::Baby, SheetState::Work) => BABY_WORK,
        (Stage::Baby, SheetState::Think) => BABY_THINK,
        (Stage::Baby, SheetState::Happy) => BABY_HAPPY,
        (Stage::Baby, SheetState::Sleep) => BABY_SLEEP,
        (Stage::Baby, SheetState::Alert) => BABY_ALERT,
        (Stage::Baby, SheetState::Success) => BABY_SUCCESS,
        (Stage::Baby, SheetState::Error) => BABY_ERROR,
        (Stage::Juvenile, SheetState::Idle) => JUVENILE_IDLE,
        (Stage::Juvenile, SheetState::Work) => JUVENILE_WORK,
        (Stage::Juvenile, SheetState::Think) => JUVENILE_THINK,
        (Stage::Juvenile, SheetState::Happy) => JUVENILE_HAPPY,
        (Stage::Juvenile, SheetState::Sleep) => JUVENILE_SLEEP,
        (Stage::Juvenile, SheetState::Alert) => JUVENILE_ALERT,
        (Stage::Juvenile, SheetState::Success) => JUVENILE_SUCCESS,
        (Stage::Juvenile, SheetState::Error) => JUVENILE_ERROR,
        (Stage::Adult, SheetState::Idle) => ADULT_IDLE,
        (Stage::Adult, SheetState::Work) => ADULT_WORK,
        (Stage::Adult, SheetState::Think) => ADULT_THINK,
        (Stage::Adult, SheetState::Happy) => ADULT_HAPPY,
        (Stage::Adult, SheetState::Sleep) => ADULT_SLEEP,
        (Stage::Adult, SheetState::Alert) => ADULT_ALERT,
        (Stage::Adult, SheetState::Success) => ADULT_SUCCESS,
        (Stage::Adult, SheetState::Error) => ADULT_ERROR,
    }
}
