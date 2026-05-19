use super::super::model::Stage;
use super::symbols::{eye_symbol, rarity_badge, species_mark, species_variant, variant_symbol};
use super::types::{
    CardArtFamily, SpeciesSeed, VivlingCardArt, VivlingRarity, VivlingSpeciesDefinition,
    VivlingStageCardArt,
};

pub(crate) fn card_art_for_species(
    species: &VivlingSpeciesDefinition,
    stage: Stage,
    width_hint: usize,
) -> VivlingCardArt {
    let stage_art = match stage {
        Stage::Baby => &species.card_baby,
        Stage::Juvenile => &species.card_juvenile,
        Stage::Adult => &species.card_adult,
    };
    let lines = if width_hint < 88 {
        stage_art.narrow_lines.clone()
    } else {
        stage_art.wide_lines.clone()
    };
    VivlingCardArt { lines }
}

pub(super) fn stage_card_art(
    seed: &SpeciesSeed,
    rarity: VivlingRarity,
    stage: Stage,
) -> VivlingStageCardArt {
    if let Some(lines) = species_card_art_lines(seed.id, stage) {
        let lines = lines
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<String>>();
        return VivlingStageCardArt {
            narrow_lines: lines.clone(),
            wide_lines: lines,
        };
    }

    let narrow = card_art_lines(seed.family, stage, true);
    let wide = card_art_lines(seed.family, stage, false);
    VivlingStageCardArt {
        narrow_lines: personalize_card_lines(seed, rarity, stage, narrow),
        wide_lines: personalize_card_lines(seed, rarity, stage, wide),
    }
}

fn species_card_art_lines(id: &str, stage: Stage) -> Option<Vec<&'static str>> {
    match (id, stage) {
        ("syllo", Stage::Baby) => Some(vec![
            "          .-o-.",
            "         /(   )\\",
            "          /_<_\\",
            "          /   \\",
            "         /_/ \\_\\",
        ]),
        ("syllo", Stage::Juvenile) => Some(vec![
            "          .-o-.",
            "        _/(   )\\_",
            "       /  /_<_\\  \\",
            "          / | \\",
            "         /_/ \\_\\",
        ]),
        ("syllo", Stage::Adult) => Some(vec![
            "          \\.-o-./",
            "        .--(   )--.",
            "       /___/_<_\\___\\",
            "           / | \\",
            "         _/  |  \\_",
            "        /___/ \\___\\",
        ]),
        ("orchestra", Stage::Baby) => Some(vec![
            "          .-o-.",
            "        o-(   )-o",
            "           /_\\",
            "          /___\\",
            "           / \\",
        ]),
        ("orchestra", Stage::Juvenile) => Some(vec![
            "        o   .-o-.   o",
            "         \\_/(   )\\_/",
            "             /|\\",
            "            /_|_\\",
            "            /   \\",
        ]),
        ("orchestra", Stage::Adult) => Some(vec![
            "       o    .-o-.    o",
            "        \\_.-(   )-._/",
            "            /|||\\",
            "          _/ |_| \\_",
            "         /__/   \\__\\",
        ]),
        ("chronosworn", Stage::Baby) => Some(vec![
            "          .-o-.",
            "          ( | )",
            "          -/_\\-",
            "           / \\",
        ]),
        ("chronosworn", Stage::Juvenile) => Some(vec![
            "         o-.-o",
            "        --/|\\--",
            "         o/ \\o",
            "          /_\\",
        ]),
        ("chronosworn", Stage::Adult) => Some(vec![
            "          o-.-o",
            "       .--/| |\\--.",
            "      o__/ | | \\__o",
            "          _|O|_",
            "         /__|__\\",
            "        _/     \\_",
        ]),
        _ => None,
    }
}

fn personalize_card_lines(
    seed: &SpeciesSeed,
    rarity: VivlingRarity,
    stage: Stage,
    base_lines: Vec<&'static str>,
) -> Vec<String> {
    let variant = species_variant(seed.id, seed.name, stage);
    let horn = variant_symbol(variant);
    let eye = eye_symbol(variant + 1);
    let sig = rarity_badge(rarity, variant + 2);
    let mark = species_mark(seed.id, seed.name);
    let mut lines = base_lines
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    if let Some(first) = lines.first_mut() {
        let trimmed = first.trim_end();
        *first = format!("{trimmed}  {sig}{mark}");
    }
    if lines.len() > 1 {
        let title = format!(" {mark} {horn}{eye} {sig}");
        lines[1] = overlay_center(&lines[1], &title);
    }
    if let Some(last) = lines.last_mut() {
        let tail = format!("{mark}{}{horn}", eye);
        let trimmed = last.trim_end();
        *last = format!("{trimmed}  {tail}");
    }
    lines
}

fn overlay_center(base: &str, overlay: &str) -> String {
    let mut chars = base.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return overlay.to_string();
    }
    let overlay_chars = overlay.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(overlay_chars.len()) / 2;
    for (idx, ch) in overlay_chars.into_iter().enumerate() {
        if let Some(slot) = chars.get_mut(start + idx) {
            *slot = ch;
        }
    }
    chars.into_iter().collect()
}

fn card_art_lines(family: CardArtFamily, stage: Stage, compact: bool) -> Vec<&'static str> {
    match (family, stage, compact) {
        (CardArtFamily::Bud, Stage::Baby, true) => vec![
            "        . . .",
            "      .-o-o-.",
            "    .'  /_\\  '.",
            "   /___/___\\___\\",
            "      /  _  \\",
            "     /__/ \\__\\",
            "      /_/ \\_\\",
        ],
        (CardArtFamily::Bud, Stage::Juvenile, true) => vec![
            "        . . . .",
            "      .-[o^']-.",
            "    .'  /___\\  '.",
            "   /___/_____\\___\\",
            "   |   |  _  |   |",
            "   |___| / \\ |___|",
            "     /_/ |_| \\_\\",
        ],
        (CardArtFamily::Bud, Stage::Adult, true) => vec![
            "       . . . . .",
            "     .-[o*^]-[ ]",
            "   .'  /_____\\  '.",
            "  /___/___ ___\\___\\",
            "  |   |  /_\\  |   |",
            "  |___| /___\\ |___|",
            "    /_/ /_|_\\ \\_\\",
        ],
        (CardArtFamily::Bud, Stage::Baby, false) => vec![
            "                 . . .",
            "            .---o-o---.",
            "         .-'    /_\\    '-.",
            "       .'     _/___\\_     '.",
            "      /______/__ _ __\\______\\",
            "         /_/   /_/ \\_\\   \\_\\",
            "        /_/     /___\\     \\_\\",
        ],
        (CardArtFamily::Bud, Stage::Juvenile, false) => vec![
            "                  . . . .",
            "            .---[o^']---.",
            "         .-'     /___\\     '-.",
            "       .'      _/_____\\_      '.",
            "      /______/___ _ ___\\______\\",
            "      |      |  /_\\ /_\\  |      |",
            "      |______| /___V___\\ |______|",
            "         /_/   /_/   \\_\\   \\_\\",
        ],
        (CardArtFamily::Bud, Stage::Adult, false) => vec![
            "                   . . . . .",
            "            .---[ o*^ ]---.",
            "         .-'      /___\\      '-.",
            "       .'       _/_____|\\_       '.",
            "      /_______/___ ___ ___\\_______\\",
            "      |       |  /_\\ /_\\  |       |",
            "      |       | /___V___\\ |       |",
            "      |_______|/__/   \\__\\|_______|",
        ],
        (CardArtFamily::Shell, Stage::Baby, true) => vec![
            "        _ . _",
            "      .-[___]-.",
            "    .'  /___\\  '.",
            "   /___/_____\\___\\",
            "      |  _  |",
            "      |_/ \\_|",
            "       /___\\",
        ],
        (CardArtFamily::Shell, Stage::Juvenile, true) => vec![
            "       _ . _ .",
            "     .-[[_]]--.",
            "   .'  /_____\\  '.",
            "  /___/___ ___\\___\\",
            "  |   |  /_\\  |   |",
            "  |___| /___\\ |___|",
            "    /_/ |_| \\_\\",
        ],
        (CardArtFamily::Shell, Stage::Adult, true) => vec![
            "      _ . _ . _",
            "    .-[[___]]--.",
            "  .'  /_______\\  '.",
            " /___/___ _ ___\\___\\",
            " |   |  /_\\ /_\\ |   |",
            " |___| /___V___\\|___|",
            "   /_/ /__/ \\__\\\\_\\",
        ],
        (CardArtFamily::Shell, Stage::Baby, false) => vec![
            "                 _ . _",
            "            .---[___]---.",
            "         .-'     /___\\     '-.",
            "       .'      _/_____\\_      '.",
            "      /______/___ _ ___\\______\\",
            "         |      /_\\ /_\\      |",
            "         |_____/___V___\\_____|",
        ],
        (CardArtFamily::Shell, Stage::Juvenile, false) => vec![
            "                 _ . _ .",
            "            .---[[___]]---.",
            "         .-'      /___\\      '-.",
            "       .'       _/_____\\_       '.",
            "      /_______/___ _ ___\\_______\\",
            "      |       |  /_\\ /_\\  |       |",
            "      |_______| /___V___\\ |_______|",
            "         /_/     /_/ \\_\\     \\_\\",
        ],
        (CardArtFamily::Shell, Stage::Adult, false) => vec![
            "                  _ . _ . _",
            "            .---[[[___]]]---.",
            "         .-'       /___\\       '-.",
            "       .'        _/_____\\_        '.",
            "      /________/___ ___ ___\\________\\",
            "      |        |  /_\\ /_\\  |        |",
            "      |        | /___V___\\ |        |",
            "      |________|/__/   \\__\\|________|",
        ],
        (CardArtFamily::Spark, Stage::Baby, true) => vec![
            "        . * .",
            "      .-\\ ^ /-.",
            "    .'   \\_/   '.",
            "   /___ /_*_\\ ___\\",
            "      /  |  \\",
            "     /__/ \\__\\",
            "       /___\\",
        ],
        (CardArtFamily::Spark, Stage::Juvenile, true) => vec![
            "       . * . *",
            "     .-\\ ^*^ /-.",
            "   .'   \\___/   '.",
            "  /___ /__*__\\ ___\\",
            "  |   |  /_\\  |   |",
            "  |___| /_|_\\ |___|",
            "    /_/   |   \\_\\",
        ],
        (CardArtFamily::Spark, Stage::Adult, true) => vec![
            "      . * . * .",
            "    .-\\ ^*+^ /-.",
            "  .'   \\_____ /  '.",
            " /___ /__***__\\ ___\\",
            " |   |  /_\\ /_\\ |   |",
            " |___| /_|_V_|_\\|___|",
            "   /_/   / \\   \\_\\",
        ],
        (CardArtFamily::Spark, Stage::Baby, false) => vec![
            "                 . * .",
            "            .---\\ ^ /---.",
            "         .-'     \\_/     '-.",
            "       .'       _/*\\_       '.",
            "      /______/__/___\\__\\______\\",
            "         /_/     / | \\     \\_\\",
            "        /_/     /_/ \\_\\     \\_\\",
        ],
        (CardArtFamily::Spark, Stage::Juvenile, false) => vec![
            "                 . * . *",
            "            .---\\ ^*^ /---.",
            "         .-'      \\_/      '-.",
            "       .'        _/**\\_        '.",
            "      /________/__/___\\__\\________\\",
            "      |        |  /_\\ /_\\  |        |",
            "      |________| /_|_V_|_\\ |________|",
            "         /_/      / | \\      \\_\\",
        ],
        (CardArtFamily::Spark, Stage::Adult, false) => vec![
            "                  . * . * .",
            "            .---\\ ^*+^ /---.",
            "         .-'       \\_/       '-.",
            "       .'        _/***\\_        '.",
            "      /________/__/___\\__\\________\\",
            "      |        |  /_\\ /_\\  |        |",
            "      |        | /_|_V_|_\\ |        |",
            "      |________|/__/ | \\__\\|________|",
        ],
        (CardArtFamily::Bloom, Stage::Baby, true) => vec![
            "        ~ ~ ~",
            "      .-(~o)-.",
            "    .'  /_~_\\  '.",
            "   /___/_____\\___\\",
            "      /  |  \\",
            "     /__/ \\__\\",
            "       /_~_\\",
        ],
        (CardArtFamily::Bloom, Stage::Juvenile, true) => vec![
            "       ~ ~ ~ ~",
            "     .-(~*~)-.",
            "   .'  /_~~~_\\  '.",
            "  /___/_______\\___\\",
            "  |   |  /_\\  |   |",
            "  |___| /___\\ |___|",
            "    /_/ /~~~\\ \\_\\",
        ],
        (CardArtFamily::Bloom, Stage::Adult, true) => vec![
            "      ~ ~ ~ ~ ~",
            "    .-(~^*~)-.",
            "  .'  /_~~~~~_\\  '.",
            " /___/_________\\___\\",
            " |   |  /_\\ /_\\ |   |",
            " |___| /___V___\\|___|",
            "   /_/ /_~~~_\\ \\_\\",
        ],
        (CardArtFamily::Bloom, Stage::Baby, false) => vec![
            "                 ~ ~ ~",
            "            .---(~o)---.",
            "         .-'     /~\\     '-.",
            "       .'      _/~~~\\_      '.",
            "      /______/__/___\\__\\______\\",
            "         /_/    /_~_\\    \\_\\",
            "        /_/    /_/ \\_\\    \\_\\",
        ],
        (CardArtFamily::Bloom, Stage::Juvenile, false) => vec![
            "                 ~ ~ ~ ~",
            "            .---(~*~)---.",
            "         .-'      /~\\      '-.",
            "       .'       _/~~~\\_       '.",
            "      /_______/__/___\\__\\_______\\",
            "      |       |  /_\\ /_\\  |       |",
            "      |_______| /___V___\\ |_______|",
            "         /_/     /~~~\\     \\_\\",
        ],
        (CardArtFamily::Bloom, Stage::Adult, false) => vec![
            "                  ~ ~ ~ ~ ~",
            "            .---(~^*~)---.",
            "         .-'       /~\\       '-.",
            "       .'        _/~~~\\_        '.",
            "      /________/__/___\\__\\________\\",
            "      |        |  /_\\ /_\\  |        |",
            "      |        | /___V___\\ |        |",
            "      |________|/__/~~~\\__\\|________|",
        ],
        (CardArtFamily::Prism, Stage::Baby, true) => vec![
            "        /\\ /\\",
            "       /  V  \\",
            "      <  <>  >",
            "       \\ /\\ /",
            "        /__\\",
            "       /_  _\\",
            "        /_/\\_\\",
        ],
        (CardArtFamily::Prism, Stage::Juvenile, true) => vec![
            "       /\\ /\\ /\\",
            "      /  V   V \\",
            "     <  <> <>  >",
            "      \\  /_\\  /",
            "      /_/___\\_\\",
            "      \\ \\___/ /",
            "       \\_/ \\_/",
        ],
        (CardArtFamily::Prism, Stage::Adult, true) => vec![
            "      /\\ /\\ /\\ /\\",
            "     /  V   V   V \\",
            "    <  <> <> <>  >",
            "     \\  /_\\ /_\\  /",
            "      \\_/___ ___\\/",
            "      / /___V___\\ \\",
            "      \\_\\   |   /_/",
        ],
        (CardArtFamily::Prism, Stage::Baby, false) => vec![
            "                 /\\  /\\",
            "               _/  \\/  \\_",
            "              <   <> <>  >",
            "               \\   /\\   /",
            "                \\_/__\\_/",
            "                 /_/\\_\\",
            "                /_/  \\_\\",
        ],
        (CardArtFamily::Prism, Stage::Juvenile, false) => vec![
            "                /\\  /\\  /\\",
            "              _/  \\/  \\/  \\_",
            "             <   <>  <>  <>  >",
            "              \\   /\\  /\\   /",
            "               \\_/__\\/__\\_/",
            "               /_/___ ___\\_\\",
            "               \\_\\___V___/_/",
            "                 /_/  \\_\\",
        ],
        (CardArtFamily::Prism, Stage::Adult, false) => vec![
            "               /\\  /\\  /\\  /\\",
            "             _/  \\/  \\/  \\/  \\_",
            "            <   <>  <>  <>  <>  >",
            "             \\   /\\  /\\  /\\   /",
            "              \\_/__\\/__\\/__\\_/",
            "              /_/___ ___ ___\\_\\",
            "              \\_\\___V___V___/_/",
            "                /_/     \\_\\",
        ],
        (CardArtFamily::Crest, Stage::Baby, true) => vec![
            "        .:::.",
            "      .::^^::.",
            "     <  /__\\  >",
            "      \\  ||  /",
            "       \\_||_/",
            "        /__\\",
            "       /_/\\_\\",
        ],
        (CardArtFamily::Crest, Stage::Juvenile, true) => vec![
            "       .:::::.",
            "     .::^^^^::.",
            "    <  /____\\  >",
            "     \\  |__|  /",
            "      \\ /__\\ /",
            "      /_/  \\_\\",
            "      \\_\\  /_/",
        ],
        (CardArtFamily::Crest, Stage::Adult, true) => vec![
            "      .:::::::.",
            "    .::^^^^^^^::.",
            "   <  /_______\\  >",
            "    \\  |__ __|  /",
            "     \\ /__V__\\ /",
            "     /_/ / \\ \\_\\",
            "     \\_\\/___\\/_/",
        ],
        (CardArtFamily::Crest, Stage::Baby, false) => vec![
            "                 .:::::.",
            "             .::^^^^^^^::.",
            "            <   /_____\\   >",
            "             \\   |___|   /",
            "              \\  /___\\  /",
            "               \\/_/ \\_\\/",
            "                /_/ \\_\\",
        ],
        (CardArtFamily::Crest, Stage::Juvenile, false) => vec![
            "                .:::::::.",
            "            .::^^^^^^^^^::.",
            "           <   /_______\\   >",
            "            \\   |__ __|   /",
            "             \\  /__V__\\  /",
            "              \\/_/   \\_\\/",
            "              /_/_____\\_\\",
            "              \\_\\     /_/",
        ],
        (CardArtFamily::Crest, Stage::Adult, false) => vec![
            "               .:::::::::.",
            "           .::^^^^^^^^^^^::.",
            "          <   /_________\\   >",
            "           \\   |__ ___|   /",
            "            \\  /__V__\\  /",
            "             \\/_/   \\_\\/",
            "             /_/_____\\_\\",
            "             \\_\\/___\\/_/",
        ],
        (CardArtFamily::Weaver, Stage::Baby, true) => vec![
            "        /\\/\\/\\",
            "       <  wm  >",
            "      /_/||\\_\\",
            "       \\ || /",
            "        \\||/",
            "        /__\\",
            "       /_/\\_\\",
        ],
        (CardArtFamily::Weaver, Stage::Juvenile, true) => vec![
            "       /\\/\\/\\/\\",
            "      <  wmmw  >",
            "     /_/||||\\_\\",
            "      \\ |||| /",
            "      /_/__\\_\\",
            "      \\_\\__/ /",
            "       /_/\\_\\",
        ],
        (CardArtFamily::Weaver, Stage::Adult, true) => vec![
            "      /\\/\\/\\/\\/\\",
            "     <  wmmwm  >",
            "    /_/||||||\\_\\",
            "     \\ |||||| /",
            "      \\_/__\\_/",
            "      /_/  \\_\\",
            "      \\_\\__/ /",
        ],
        (CardArtFamily::Weaver, Stage::Baby, false) => vec![
            "                 /\\/\\/\\",
            "              __<  wm  >__",
            "             /_/\\||||/\\_\\",
            "              \\  ||||  /",
            "               \\ /__\\ /",
            "                /_/\\_\\",
            "               /_/  \\_\\",
        ],
        (CardArtFamily::Weaver, Stage::Juvenile, false) => vec![
            "                /\\/\\/\\/\\",
            "             __<  wmmw  >__",
            "            /_/\\||||||/\\_\\",
            "             \\  ||||||  /",
            "              \\ /____\\ /",
            "              /_/____\\_\\",
            "              \\_\\    /_/",
            "                /_/\\_\\",
        ],
        (CardArtFamily::Weaver, Stage::Adult, false) => vec![
            "               /\\/\\/\\/\\/\\",
            "            __<  wmmwm  >__",
            "           /_/\\||||||||/\\_\\",
            "            \\  ||||||||  /",
            "             \\ /______\\ /",
            "             /_/______\\_\\",
            "             \\_\\  /\\  /_/",
            "               \\_/  \\_/",
        ],
        (CardArtFamily::Shade, Stage::Baby, true) => vec![
            "        .   .",
            "      .'.-.-.'.",
            "     /  '...'  \\",
            "    /___/___\\___\\",
            "      /  |  \\",
            "      \\  |  /",
            "       \\/_\\/",
        ],
        (CardArtFamily::Shade, Stage::Juvenile, true) => vec![
            "       .  .  .",
            "     .'.-...-.'.",
            "    /  '..^..'  \\",
            "   /___/_____\\___\\",
            "   |   |  _  |   |",
            "   \\___| / \\ |___/",
            "      /_/ \\_\\",
        ],
        (CardArtFamily::Shade, Stage::Adult, true) => vec![
            "      .  .  .  .",
            "    .'.-.....-.'.",
            "   /  '..^ ^..'  \\",
            "  /___/_______\\___\\",
            "  |   |  /_\\  |   |",
            "  \\___| /___\\ |___/",
            "     /_/ / \\ \\_\\",
        ],
        (CardArtFamily::Shade, Stage::Baby, false) => vec![
            "                 .   .",
            "            .---'.-.-.'---.",
            "         .-'     '..'     '-.",
            "       .'       _/___\\_       '.",
            "      /______/__/___\\__\\______\\",
            "         \\      / | \\      /",
            "          \\____/  |  \\____/",
        ],
        (CardArtFamily::Shade, Stage::Juvenile, false) => vec![
            "                .  .  .",
            "            .---'.-.-.'---.",
            "         .-'      '..'      '-.",
            "       .'        _/___\\_        '.",
            "      /________/__/___\\__\\________\\",
            "      |        |  /_\\  |        |",
            "      \\________| /___\\ |________/",
            "           /_/    |    \\_\\",
        ],
        (CardArtFamily::Shade, Stage::Adult, false) => vec![
            "               .  .  .  .",
            "           .---'.-.-.-.'---.",
            "        .-'       '..'       '-.",
            "      .'         _/___\\_         '.",
            "     /_________/__/___\\__\\_________\\",
            "     |         |  /_\\ /_\\  |         |",
            "     \\_________| /___V___\\ |_________/",
            "          /_/      |      \\_\\",
        ],
        (CardArtFamily::Crown, Stage::Baby, true) => vec![
            "        * * *",
            "      .-^^^^-.",
            "     /  /**\\  \\",
            "    /__/____\\__\\",
            "      /  ||  \\",
            "      \\__||__/",
            "        /__\\",
        ],
        (CardArtFamily::Crown, Stage::Juvenile, true) => vec![
            "       * * * *",
            "     .-^^^^^^-.",
            "    /  /****\\  \\",
            "   /__/______\\__\\",
            "   |  |  __  |  |",
            "   |__| /__\\ |__|",
            "     /_/ /\\ \\_\\",
        ],
        (CardArtFamily::Crown, Stage::Adult, true) => vec![
            "      * * * * *",
            "    .-^^^^^^^^-.",
            "   /  /******\\  \\",
            "  /__/________\\__\\",
            "  |  |  /__\\  |  |",
            "  |__| /____\\ |__|",
            "    /_/ /_\\_\\ \\_\\",
        ],
        (CardArtFamily::Crown, Stage::Baby, false) => vec![
            "                 * * *",
            "            .---^^^^---.",
            "         .-'    /**\\    '-.",
            "       .'      /____\\      '.",
            "      /______/__ __ __\\______\\",
            "         |      /_||_\\      |",
            "         |_____/__||__\\_____|",
        ],
        (CardArtFamily::Crown, Stage::Juvenile, false) => vec![
            "                * * * *",
            "            .---^^^^^^---.",
            "         .-'     /**\\     '-.",
            "       .'       /____\\       '.",
            "      /_______/___ __ ___\\_______\\",
            "      |       |  /_||_\\  |       |",
            "      |_______| /__||__\\ |_______|",
            "         /_/      /\\      \\_\\",
        ],
        (CardArtFamily::Crown, Stage::Adult, false) => vec![
            "               * * * * *",
            "            .---^^^^^^^^---.",
            "         .-'      /****\\      '-.",
            "       .'        /______\\        '.",
            "      /________/___ __ ___\\________\\",
            "      |        |  /_||_\\  |        |",
            "      |        | /__||__\\ |        |",
            "      |________|/___||___\\|________|",
        ],
    }
}
