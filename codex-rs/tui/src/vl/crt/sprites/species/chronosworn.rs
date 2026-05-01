use super::super::super::director::CrtMode;
use super::super::super::frame::Frame;
use super::super::super::palette::Slot;
use crate::vivling::Stage;

const BODY: [Slot; 3] = [Slot::Signal, Slot::Face, Slot::Signal];
const ALERT: [Slot; 3] = [Slot::Alert, Slot::Face, Slot::Alert];

pub(crate) fn art_for(stage: Stage, mode: CrtMode, tick: u64) -> Frame {
    let state = super::super::sheet::state_for_mode(mode);
    if let Some(rows) = super::super::sheet::frame("chronosworn", stage, state) {
        return Frame::new(*rows, slots_for(mode));
    }
    legacy_art_for(stage, mode, tick)
}

fn slots_for(mode: CrtMode) -> [Slot; 3] {
    match mode {
        CrtMode::Alert => ALERT,
        _ => BODY,
    }
}

fn legacy_art_for(stage: Stage, mode: CrtMode, tick: u64) -> Frame {
    match mode {
        CrtMode::Working => match tick % 3 {
            0 => Frame::new(["  < o >  >>  ", " --(@)--===  ", "   /|\\       "], BODY),
            1 => Frame::new([" >> o >>     ", " --(@)--==   ", "   /|\\       "], BODY),
            _ => Frame::new(["  << o >     ", " ==--(@)--   ", "   /|\\       "], BODY),
        },
        CrtMode::Thinking => Frame::new(["  < o >  ?   ", "   (@)       ", "  ./:\\.      "], BODY),
        CrtMode::Alert => Frame::new([" !< o >!     ", " --(@)--     ", " ! /|\\ !     "], ALERT),
        CrtMode::Tired => Frame::new(["  < z >      ", "   (@)       ", " __/|\\__     "], BODY),
        CrtMode::Hungry => Frame::new(["  < o >      ", "   (@)  *    ", "   /|\\       "], BODY),
        CrtMode::Idle => match (stage, tick % 6 == 0) {
            (Stage::Baby, true) => {
                Frame::new(["  ( o )      ", "   (@)       ", "   / \\       "], BODY)
            }
            (Stage::Baby, false) => {
                Frame::new(["  < o >      ", "   (@)       ", "   / \\       "], BODY)
            }
            (Stage::Juvenile, true) => {
                Frame::new(["  ( o )      ", " --(@)--     ", "   /|\\       "], BODY)
            }
            (Stage::Juvenile, false) => {
                Frame::new(["  < o >      ", " --(@)--     ", "   /|\\       "], BODY)
            }
            (Stage::Adult, true) => {
                Frame::new([" <( o )>     ", " --(@)--     ", "  _/|\\_      "], BODY)
            }
            (Stage::Adult, false) => {
                Frame::new([" << o >>     ", " --(@)--     ", "  _/|\\_      "], BODY)
            }
        },
    }
}
