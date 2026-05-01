use super::super::director::CrtMode;
use super::super::frame::Frame;
use super::super::palette::Slot;

const ROW_SLOTS: [Slot; 3] = [Slot::Signal, Slot::Face, Slot::Signal];
const ALERT_ROW_SLOTS: [Slot; 3] = [Slot::Alert, Slot::Face, Slot::Signal];

pub(crate) fn art_for(mode: CrtMode, tick: u64) -> Frame {
    match mode {
        CrtMode::Idle => idle(tick),
        CrtMode::Thinking => thinking(tick),
        CrtMode::Working => working(tick),
        CrtMode::Alert => alert(tick),
        CrtMode::Tired => tired(tick),
        CrtMode::Hungry => hungry(tick),
    }
}

fn idle(tick: u64) -> Frame {
    if tick % 8 == 0 {
        Frame::new(
            ["   .-.__.-.   ", "  (  -  -  )  ", "   `-.__.-'   "],
            ROW_SLOTS,
        )
    } else {
        Frame::new(
            ["   .-.__.-.   ", "  (  o  o  )  ", "   `-.__.-'   "],
            ROW_SLOTS,
        )
    }
}

fn thinking(tick: u64) -> Frame {
    match tick % 4 {
        0 => Frame::new(
            ["     .        ", "  (  o  o  )  ", "     . .      "],
            ROW_SLOTS,
        ),
        1 => Frame::new(
            ["     . o      ", "  (  o  o  )  ", "       . .    "],
            ROW_SLOTS,
        ),
        2 => Frame::new(
            ["     . o O    ", "  (  -  o  )  ", "    . .       "],
            ROW_SLOTS,
        ),
        _ => Frame::new(
            ["   O o .      ", "  (  o  -  )  ", "      . .     "],
            ROW_SLOTS,
        ),
    }
}

fn working(tick: u64) -> Frame {
    match tick % 4 {
        0 => Frame::new(
            ["   /|_    *   ", "  ( >_> )==   ", "   /___\\      "],
            ROW_SLOTS,
        ),
        1 => Frame::new(
            ["    _|\\   +   ", "  ( >_> ) =   ", "   /___\\      "],
            ROW_SLOTS,
        ),
        2 => Frame::new(
            ["   /|_    x   ", "  ( <_< )==   ", "   /___\\      "],
            ROW_SLOTS,
        ),
        _ => Frame::new(
            ["    _|\\   +   ", "  ( >_> ) =   ", "   /___\\      "],
            ROW_SLOTS,
        ),
    }
}

fn alert(tick: u64) -> Frame {
    if tick % 2 == 0 {
        Frame::new(
            ["   \\  !  /    ", "  (  O  O  )  ", "    /_^_\\     "],
            ALERT_ROW_SLOTS,
        )
    } else {
        Frame::new(
            ["   -- ! --    ", "  (  O  O  )  ", "    /_^_\\     "],
            ALERT_ROW_SLOTS,
        )
    }
}

fn tired(tick: u64) -> Frame {
    if tick % 3 == 0 {
        Frame::new(
            ["      z       ", "  (  -  -  )  ", "    /___\\     "],
            ROW_SLOTS,
        )
    } else {
        Frame::new(
            ["    z   z     ", "  (  -  -  )  ", "    /___\\     "],
            ROW_SLOTS,
        )
    }
}

fn hungry(tick: u64) -> Frame {
    if tick % 2 == 0 {
        Frame::new(
            ["    .---.     ", "  (  o  o  )  ", "    \\_v_/     "],
            ROW_SLOTS,
        )
    } else {
        Frame::new(
            ["    .---.     ", "  (  o  o  )  ", "    \\___/     "],
            ROW_SLOTS,
        )
    }
}
