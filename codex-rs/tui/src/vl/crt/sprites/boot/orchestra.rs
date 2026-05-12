use super::BootSprite;

pub(crate) const BOOT: BootSprite = BootSprite {
    rows_closed: [
        "   .~~~~~~~.   ",
        "  ( ~ ~ ~ ~ )  ",
        " { -. ~ ~ .- } ",
        " {  ' ~~~ '  } ",
        "  ( ~~~~~~~ )  ",
        "   '~~|~|~~'   ",
        "    /~~~~~\\    ",
        "   ~ ~ * ~ ~   ",
    ],
    rows_open: [
        "   .~~~~~~~.   ",
        "  ( ~ ~ ~ ~ )  ",
        " { o  ~ ~  o } ",
        " {  '. ~ .'  } ",
        "  ( ~ \\_/ ~ )  ",
        "   '~~|~|~~'   ",
        "    /~~~~~\\    ",
        "   ~ ~ * ~ ~   ",
    ],
    greeting: "vivling.orchestra online",
};
