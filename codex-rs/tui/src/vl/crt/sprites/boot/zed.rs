use super::BootSprite;

pub(crate) const BOOT: BootSprite = BootSprite {
    rows_closed: [
        "   .=======.   ",
        "  /| _____ |\\  ",
        " | | -. .- | | ",
        " | |  ' '  | | ",
        "  \\| _____ |/  ",
        "   '==[ ]=='   ",
        "    /| | |\\    ",
        "   = '=' = =   ",
    ],
    rows_open: [
        "   .=======.   ",
        "  /| _____ |\\  ",
        " | | o   o | | ",
        " | |   ^   | | ",
        "  \\| \\___/ |/  ",
        "   '==[ ]=='   ",
        "    /| | |\\    ",
        "   = '=' = =   ",
    ],
    greeting: "vivling.zed online",
};
