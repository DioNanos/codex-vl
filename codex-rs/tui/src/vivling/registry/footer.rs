use super::super::model::Stage;
use super::symbols::species_variant;
use super::types::{CardArtFamily, SpeciesSeed, VivlingRarity, VivlingSpeciesDefinition};

pub(super) fn footer_sprite_for_species(
    seed: &SpeciesSeed,
    rarity: VivlingRarity,
    stage: Stage,
) -> String {
    let base = footer_sprite(seed.family, rarity, stage);
    let accents = species_footer_accent(seed.id, seed.name, stage, rarity);
    match stage {
        Stage::Baby => format!("{base}{accents}"),
        Stage::Juvenile => format!("{accents}{base}"),
        Stage::Adult => format!("{base}{accents}"),
    }
}

pub(crate) fn active_footer_sprites_for_species(
    species: &VivlingSpeciesDefinition,
    stage: Stage,
) -> [String; 3] {
    let frames = active_footer_sprites(species.footer_family, species.rarity, stage);
    let accents = species_footer_accent(&species.id, &species.name, stage, species.rarity);
    match stage {
        Stage::Baby => [
            format!("{}{}", frames[0], accents),
            format!("{}{}", frames[1], accents),
            format!("{}{}", frames[2], accents),
        ],
        Stage::Juvenile => [
            format!("{}{}", accents, frames[0]),
            format!("{}{}", accents, frames[1]),
            format!("{}{}", accents, frames[2]),
        ],
        Stage::Adult => [
            format!("{}{}", frames[0], accents),
            format!("{}{}", frames[1], accents),
            format!("{}{}", frames[2], accents),
        ],
    }
}

fn species_footer_accent(
    id: &str,
    name: &str,
    stage: Stage,
    rarity: VivlingRarity,
) -> &'static str {
    const COMMON_MARKERS: [&str; 6] = ["`", ".", ":", "~", "'", ","];
    const RARE_MARKERS: [&str; 6] = ["*", "+", "^", "=", "!", "&"];
    const LEGENDARY_MARKERS: [&str; 6] = ["*", "#", "@", "%", "&", "$"];
    const MYTHIC_MARKERS: [&str; 6] = [">", "<", "|", "/", "\\", "0"];
    let idx = species_variant(id, name, stage) % 6;
    match rarity {
        VivlingRarity::Common => COMMON_MARKERS[idx],
        VivlingRarity::Rare => RARE_MARKERS[idx],
        VivlingRarity::Legendary => LEGENDARY_MARKERS[idx],
        VivlingRarity::Mythic => MYTHIC_MARKERS[idx],
    }
}

fn footer_sprite(family: CardArtFamily, rarity: VivlingRarity, stage: Stage) -> &'static str {
    match (family, rarity, stage) {
        (CardArtFamily::Bud, _, Stage::Baby) => "(o.)",
        (CardArtFamily::Bud, _, Stage::Juvenile) => "<o'>",
        (CardArtFamily::Bud, _, Stage::Adult) => "[o*]",
        (CardArtFamily::Shell, _, Stage::Baby) => "(_.)",
        (CardArtFamily::Shell, _, Stage::Juvenile) => "<[_]>",
        (CardArtFamily::Shell, _, Stage::Adult) => "[/_\\\\]",
        (CardArtFamily::Spark, _, Stage::Baby) => "(*.)",
        (CardArtFamily::Spark, _, Stage::Juvenile) => "<*^>",
        (CardArtFamily::Spark, _, Stage::Adult) => "[*+]",
        (CardArtFamily::Bloom, _, Stage::Baby) => "(~.)",
        (CardArtFamily::Bloom, _, Stage::Juvenile) => "<~*>",
        (CardArtFamily::Bloom, _, Stage::Adult) => "[~^]",
        (CardArtFamily::Prism, _, Stage::Baby) => "<>. ",
        (CardArtFamily::Prism, _, Stage::Juvenile) => "<◇>",
        (CardArtFamily::Prism, _, Stage::Adult) => "[◇*]",
        (CardArtFamily::Crest, _, Stage::Baby) => "(::)",
        (CardArtFamily::Crest, _, Stage::Juvenile) => "<^^>",
        (CardArtFamily::Crest, _, Stage::Adult) => "[^^]",
        (CardArtFamily::Weaver, _, Stage::Baby) => "(wm)",
        (CardArtFamily::Weaver, _, Stage::Juvenile) => "<wm>",
        (CardArtFamily::Weaver, _, Stage::Adult) => "[wm]",
        (CardArtFamily::Shade, _, Stage::Baby) => "('.)",
        (CardArtFamily::Shade, _, Stage::Juvenile) => "<'.>",
        (CardArtFamily::Shade, _, Stage::Adult) => "[`^]",
        (CardArtFamily::Crown, _, Stage::Baby) => "(**)",
        (CardArtFamily::Crown, _, Stage::Juvenile) => "{**}",
        (CardArtFamily::Crown, _, Stage::Adult) => "{**^}",
    }
}

fn active_footer_sprites(
    family: CardArtFamily,
    _rarity: VivlingRarity,
    stage: Stage,
) -> [&'static str; 3] {
    match (family, stage) {
        (CardArtFamily::Bud, Stage::Baby) => ["(o.)", "(.o)", "(o')"],
        (CardArtFamily::Bud, Stage::Juvenile) => ["<o'>", "<'o>", "<o*>"],
        (CardArtFamily::Bud, Stage::Adult) => ["[o*]", "[*o]", "[o+]"],
        (CardArtFamily::Shell, Stage::Baby) => ["(_.)", "(._)", "(_:)"],
        (CardArtFamily::Shell, Stage::Juvenile) => ["<[_]>", "<[.]>", "<[:]>"],
        (CardArtFamily::Shell, Stage::Adult) => ["[/_\\\\]", "[/_/]", "[\\\\_/]"],
        (CardArtFamily::Spark, Stage::Baby) => ["(*.)", "(.*)", "(*:)"],
        (CardArtFamily::Spark, Stage::Juvenile) => ["<*^>", "<^*>", "<*>^"],
        (CardArtFamily::Spark, Stage::Adult) => ["[*+]", "[+*]", "[**]"],
        (CardArtFamily::Bloom, Stage::Baby) => ["(~.)", "(.~)", "(~:)"],
        (CardArtFamily::Bloom, Stage::Juvenile) => ["<~*>", "<*~>", "<~^>"],
        (CardArtFamily::Bloom, Stage::Adult) => ["[~^]", "[^~]", "[~*]"],
        (CardArtFamily::Prism, Stage::Baby) => ["<>. ", "<.> ", "<:* "],
        (CardArtFamily::Prism, Stage::Juvenile) => ["<◇>", "<◆>", "<◈>"],
        (CardArtFamily::Prism, Stage::Adult) => ["[◇*]", "[◆*]", "[◈*]"],
        (CardArtFamily::Crest, Stage::Baby) => ["(::)", "(:.)", "(..)"],
        (CardArtFamily::Crest, Stage::Juvenile) => ["<^^>", "<^:>", "<:^>"],
        (CardArtFamily::Crest, Stage::Adult) => ["[^^]", "[^:]", "[:^]"],
        (CardArtFamily::Weaver, Stage::Baby) => ["(wm)", "(mw)", "(w:)"],
        (CardArtFamily::Weaver, Stage::Juvenile) => ["<wm>", "<mw>", "<w~>"],
        (CardArtFamily::Weaver, Stage::Adult) => ["[wm]", "[mw]", "[w~]"],
        (CardArtFamily::Shade, Stage::Baby) => ["('.)", "(.`)", "(':)"],
        (CardArtFamily::Shade, Stage::Juvenile) => ["<'.>", "<.`>", "<':>"],
        (CardArtFamily::Shade, Stage::Adult) => ["[`^]", "[^`]", "[`:]"],
        (CardArtFamily::Crown, Stage::Baby) => ["(**)", "(*:)", "(*)*"],
        (CardArtFamily::Crown, Stage::Juvenile) => ["{**}", "{*:}", "{*^}"],
        (CardArtFamily::Crown, Stage::Adult) => ["{**^}", "{*^*}", "{^**}"],
    }
}
