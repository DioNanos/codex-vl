use super::SheetState;
use crate::vivling::Stage;

const BABY_IDLE: [&str; 3] = ["   .-.   ", "  ( o )  ", "  -/_\\-  "];

const BABY_WORK: [&str; 3] = ["   .-.   ", "  ( o>)  ", "  -/_\\-  "];

const BABY_THINK: [&str; 3] = ["   .-.?  ", "  ( o )  ", "  -/_\\-  "];

const BABY_HAPPY: [&str; 3] = ["   .-.*  ", "  (^o^)  ", "  -/_\\-  "];

const BABY_SLEEP: [&str; 3] = ["   .-.z  ", "  (-o-)  ", "  -/_\\-  "];

const BABY_ALERT: [&str; 3] = ["   .-.!  ", "  ( Oo)  ", "  -/_\\-  "];

const BABY_SUCCESS: [&str; 3] = ["   .-.v  ", "  (^o^)  ", "  -/_\\-  "];

const BABY_ERROR: [&str; 3] = ["   .-.x  ", "  ( xo)  ", "  -/_\\-  "];

const JUVENILE_IDLE: [&str; 3] = ["  .-o-.  ", " --/|\\-- ", "  o/ \\o  "];

const JUVENILE_WORK: [&str; 3] = ["  .-o-.  ", " =-/|\\-- ", "  o/ \\o  "];

const JUVENILE_THINK: [&str; 3] = ["  .-o-.? ", " --/|\\-- ", "  o/ \\o  "];

const JUVENILE_HAPPY: [&str; 3] = ["  .-o-.* ", " --/|\\-- ", "  o/ \\o  "];

const JUVENILE_SLEEP: [&str; 3] = ["  .-o-.z ", " --/|\\-- ", "  o/ \\o  "];

const JUVENILE_ALERT: [&str; 3] = ["  .-o-.! ", " --/|\\-- ", "  o/ \\o  "];

const JUVENILE_SUCCESS: [&str; 3] = ["  .-o-.v ", " --/|\\-- ", "  o/ \\o  "];

const JUVENILE_ERROR: [&str; 3] = ["  .-o-.x ", " --/|\\-- ", "  o/ \\o  "];

const YOUNG_ADULT_IDLE: [&str; 3] = ["  o-.-o  ", " --/|\\-- ", " o_/ \\_o "];

const YOUNG_ADULT_WORK: [&str; 3] = ["  o-.-o= ", " --/|\\-- ", " o_/ \\_o "];

const YOUNG_ADULT_THINK: [&str; 3] = ["  o-.-o? ", " --/|\\-- ", " o_/ \\_o "];

const YOUNG_ADULT_HAPPY: [&str; 3] = ["  o-.-o* ", " --/|\\-- ", " o_/ \\_o "];

const YOUNG_ADULT_SLEEP: [&str; 3] = ["  o-.-oz ", " --/|\\-- ", " o_/ \\_o "];

const YOUNG_ADULT_ALERT: [&str; 3] = ["  o-.-o! ", " --/|\\-- ", " o_/ \\_o "];

const YOUNG_ADULT_SUCCESS: [&str; 3] = ["  o-.-ov ", " --/|\\-- ", " o_/ \\_o "];

const YOUNG_ADULT_ERROR: [&str; 3] = ["  o-.-ox ", " --/|\\-- ", " o_/ \\_o "];

pub(super) fn frame(stage: Stage, state: SheetState) -> &'static [&'static str; 3] {
    match (stage, state) {
        (Stage::Baby, SheetState::Idle) => &BABY_IDLE,
        (Stage::Baby, SheetState::Work) => &BABY_WORK,
        (Stage::Baby, SheetState::Think) => &BABY_THINK,
        (Stage::Baby, SheetState::Happy) => &BABY_HAPPY,
        (Stage::Baby, SheetState::Sleep) => &BABY_SLEEP,
        (Stage::Baby, SheetState::Alert) => &BABY_ALERT,
        (Stage::Baby, SheetState::Success) => &BABY_SUCCESS,
        (Stage::Baby, SheetState::Error) => &BABY_ERROR,
        (Stage::Juvenile, SheetState::Idle) => &JUVENILE_IDLE,
        (Stage::Juvenile, SheetState::Work) => &JUVENILE_WORK,
        (Stage::Juvenile, SheetState::Think) => &JUVENILE_THINK,
        (Stage::Juvenile, SheetState::Happy) => &JUVENILE_HAPPY,
        (Stage::Juvenile, SheetState::Sleep) => &JUVENILE_SLEEP,
        (Stage::Juvenile, SheetState::Alert) => &JUVENILE_ALERT,
        (Stage::Juvenile, SheetState::Success) => &JUVENILE_SUCCESS,
        (Stage::Juvenile, SheetState::Error) => &JUVENILE_ERROR,
        (Stage::Adult, SheetState::Idle) => &YOUNG_ADULT_IDLE,
        (Stage::Adult, SheetState::Work) => &YOUNG_ADULT_WORK,
        (Stage::Adult, SheetState::Think) => &YOUNG_ADULT_THINK,
        (Stage::Adult, SheetState::Happy) => &YOUNG_ADULT_HAPPY,
        (Stage::Adult, SheetState::Sleep) => &YOUNG_ADULT_SLEEP,
        (Stage::Adult, SheetState::Alert) => &YOUNG_ADULT_ALERT,
        (Stage::Adult, SheetState::Success) => &YOUNG_ADULT_SUCCESS,
        (Stage::Adult, SheetState::Error) => &YOUNG_ADULT_ERROR,
    }
}
