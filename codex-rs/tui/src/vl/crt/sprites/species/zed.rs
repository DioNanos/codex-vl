use super::super::super::director::CrtMode;
use super::super::super::frame::Frame;
use super::super::super::palette::Slot;
use crate::vivling::Stage;

const BODY: [Slot; 3] = [Slot::Signal, Slot::Face, Slot::Signal];
const ALERT: [Slot; 3] = [Slot::Alert, Slot::Face, Slot::Alert];

pub(crate) fn art_for(stage: Stage, mode: CrtMode, tick: u64) -> Frame {
    let state = super::super::sheet::state_for_mode(mode);
    if let Some(rows) = super::super::sheet::frame("zed", stage, state) {
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
            0 => Frame::new(["  .-< >-.    ", "  |  >_  |-- ", "  '._|_.'    "], BODY),
            1 => Frame::new(["  .-< >-. -- ", "  |  >_  |   ", "  '._|_.'    "], BODY),
            _ => Frame::new([" --.-< >-.   ", "  |  >_  |   ", "  '._|_.'    "], BODY),
        },
        CrtMode::Thinking => Frame::new(["  .-< >-. ?  ", "  |  >_  |   ", "  '._|_.' .. "], BODY),
        CrtMode::Alert => Frame::new([" !.-< >-.!   ", "  |  >_  |   ", " !'._|_.'!   "], ALERT),
        CrtMode::Tired => Frame::new(["  .-< >-. z  ", "  |  -_  |   ", "  '._|_.'    "], BODY),
        CrtMode::Hungry => Frame::new(["  .-< >-.    ", "  |  >_  | * ", "  '._|_.'    "], BODY),
        CrtMode::Idle => match (stage, tick % 6 == 0) {
            (Stage::Baby, true) => {
                Frame::new(["   .->-.     ", "   | - |     ", "   '._.'     "], BODY)
            }
            (Stage::Baby, false) => {
                Frame::new(["   .->-.     ", "   | > |     ", "   '._.'     "], BODY)
            }
            (Stage::Juvenile, true) => {
                Frame::new(["  .-< >-.    ", "  |  -_  |   ", "  '._|_.'    "], BODY)
            }
            (Stage::Juvenile, false) => {
                Frame::new(["  .-< >-.    ", "  |  >_  |   ", "  '._|_.'    "], BODY)
            }
            (Stage::Adult, true) => {
                Frame::new([" .==< >==.   ", " ||  -_  ||  ", "  '._|_.'    "], BODY)
            }
            (Stage::Adult, false) => {
                Frame::new([" .==< >==.   ", " ||  >_  ||  ", "  '._|_.'    "], BODY)
            }
        },
    }
}
