use super::BootSprite;

pub(crate) const BOOT: BootSprite = BootSprite {
    rows_closed: [
        "    .--^--.    ",
        "   /  XII  \\   ",
        "  | -.   .- |  ",
        "  |IX 'o' III| ",
        "   \\  ---  /   ",
        "    '-VI-'     ",
        "    /|||\\      ",
        "   //|||\\\\    ",
    ],
    rows_open: [
        "    .--^--.    ",
        "   /  XII  \\   ",
        "  | o     o |  ",
        "  |IX  *  III| ",
        "   \\  \\_/  /   ",
        "    '-VI-'     ",
        "    /|||\\      ",
        "   //|||\\\\    ",
    ],
    greeting: "vivling.chrono online",
};
