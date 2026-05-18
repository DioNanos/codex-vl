use super::super::super::director::CrtMode;
use super::super::super::frame::Frame;
use super::super::super::palette::Slot;
use crate::vivling::Stage;

const BODY: [Slot; 3] = [Slot::Signal, Slot::Face, Slot::Signal];
const ALERT: [Slot; 3] = [Slot::Alert, Slot::Face, Slot::Alert];

pub(crate) fn art_for(stage: Stage, mode: CrtMode, tick: u64) -> Frame {
    let state = super::super::sheet::state_for_mode(mode);
    if let Some(frames) = super::super::sheet::frame("syllo", stage, state) {
        let idx = (tick as usize) % frames.len();
        return Frame::new(frames[idx], slots_for(mode));
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
            0 => Frame::new(["   </>   .   ", "  /_@_\\ ==   ", "   / \\       "], BODY),
            1 => Frame::new(["   <{}>  +   ", "  /_@_\\  =   ", "   / \\       "], BODY),
            _ => Frame::new(["   </>   *   ", "  /_@_\\ ==   ", "   / \\       "], BODY),
        },
        CrtMode::Thinking => {
            Frame::new(["    . ?      ", "   /_@_\\     ", "    / \\  ..  "], BODY)
        }
        CrtMode::Alert => Frame::new(["    ! !      ", "   /_@_\\     ", "    / \\  !   "], ALERT),
        CrtMode::Tired => Frame::new(["    z z      ", "   /_@_\\     ", "  __/ \\__    "], BODY),
        CrtMode::Hungry => Frame::new(["    ___      ", "   /_@_\\     ", "    / \\  <>  "], BODY),
        CrtMode::Idle => match (stage, tick % 6 == 0) {
            (Stage::Baby, true) => {
                Frame::new(["    /\\       ", "   /-@-\\     ", "    / \\      "], BODY)
            }
            (Stage::Baby, false) => {
                Frame::new(["    /\\       ", "   /_@_\\     ", "    / \\      "], BODY)
            }
            (Stage::Juvenile, true) => {
                Frame::new(["   _/\\_      ", "  /-@-\\  .   ", "   /|\\       "], BODY)
            }
            (Stage::Juvenile, false) => {
                Frame::new(["   _/\\_      ", "  /_@_\\  .   ", "   /|\\       "], BODY)
            }
            (Stage::Adult, true) => {
                Frame::new(["  _</>_      ", " /--@--\\ .   ", "  /_|_\\      "], BODY)
            }
            (Stage::Adult, false) => {
                Frame::new(["  _</>_      ", " /__@__\\ .   ", "  /_|_\\      "], BODY)
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cycle through `tick` must alternate the rendered frame on the animated
    /// principal states. Tick=0 and tick=1 land on adjacent frames inside the
    /// 2-frame Idle cycle.
    #[test]
    fn art_for_idle_alternates_with_tick() {
        let a = art_for(Stage::Baby, CrtMode::Idle, 0);
        let b = art_for(Stage::Baby, CrtMode::Idle, 1);
        assert_ne!(a.rows, b.rows);
    }

    /// Single-frame cycles return identical frames regardless of tick — guards
    /// against accidental modulo behaviour on `len==1` arrays in the future.
    /// Syllo Sleep is single-frame in iter 7.
    #[test]
    fn art_for_sleep_stable_across_ticks() {
        let a = art_for(Stage::Baby, CrtMode::Tired, 0);
        let b = art_for(Stage::Baby, CrtMode::Tired, 1);
        assert_eq!(a.rows, b.rows);
    }
}
