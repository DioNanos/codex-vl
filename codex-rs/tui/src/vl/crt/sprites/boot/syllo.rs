use super::BootSprite;

pub(crate) const BOOT: BootSprite = BootSprite {
    rows_closed: [
        "    .-----.    ",
        "   /       \\   ",
        "  |  -. .-  |  ",
        "  |   ' '   |  ",
        "   \\  ___  /   ",
        "    '--|--'    ",
        "      /|\\      ",
        "     / | \\     ",
    ],
    rows_open: [
        "    .-----.    ",
        "   /       \\   ",
        "  |  o   o  |  ",
        "  |    .    |  ",
        "   \\  \\_/  /   ",
        "    '--|--'    ",
        "      /|\\      ",
        "     / | \\     ",
    ],
    greeting: "vivling.syllo online",
};
