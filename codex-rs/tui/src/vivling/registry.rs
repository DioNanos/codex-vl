use std::sync::OnceLock;

use super::model::Stage;
use super::model::WorkAffinitySet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VivlingRarity {
    Common,
    Rare,
    Legendary,
}

impl VivlingRarity {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Common => "Common",
            Self::Rare => "Rare",
            Self::Legendary => "Legendary",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VivlingAvailability {
    Active,
    Reserved,
}

impl VivlingAvailability {
    pub(crate) fn is_user_visible(self) -> bool {
        matches!(self, Self::Active)
    }
}

#[derive(Clone, Copy, Debug)]
enum CardArtFamily {
    Bud,
    Shell,
    Spark,
    Bloom,
    Prism,
    Crest,
    Weaver,
    Shade,
    Crown,
}

#[derive(Clone, Copy, Debug)]
enum BiasProfile {
    BuilderResearch,
    BuilderOperator,
    Researcher,
    ReviewerResearch,
    ReviewerOperator,
    ResearcherOperator,
}

#[derive(Clone, Copy, Debug)]
struct SpeciesSeed {
    id: &'static str,
    name: &'static str,
    family: CardArtFamily,
    bias: BiasProfile,
}

const COMMON_SPECIES: &[SpeciesSeed] = &[
    SpeciesSeed {
        id: "bytebud",
        name: "Bytebud",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "glyphlet",
        name: "Glyphlet",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "stackseed",
        name: "Stackseed",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "patchling",
        name: "Patchling",
        family: CardArtFamily::Shell,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "shellsprout",
        name: "Shellsprout",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "cursorwisp",
        name: "Cursorwisp",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "diffmote",
        name: "Diffmote",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "tokenbloom",
        name: "Tokenbloom",
        family: CardArtFamily::Bloom,
        bias: BiasProfile::Researcher,
    },
    SpeciesSeed {
        id: "mergekin",
        name: "Mergekin",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "nullstar",
        name: "Nullstar",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "rootglow",
        name: "Rootglow",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "amberbyte",
        name: "Amberbyte",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "ashmote",
        name: "Ashmote",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "bitblossom",
        name: "Bitblossom",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "bitdrift",
        name: "Bitdrift",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "bloompatch",
        name: "Bloompatch",
        family: CardArtFamily::Shell,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "cloudbud",
        name: "Cloudbud",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "cloudlatch",
        name: "Cloudlatch",
        family: CardArtFamily::Shell,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "codeling",
        name: "Codeling",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "driftbud",
        name: "Driftbud",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "driftflare",
        name: "Driftflare",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "echobud",
        name: "Echobud",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "echolatch",
        name: "Echolatch",
        family: CardArtFamily::Shell,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "fablemote",
        name: "Fablemote",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "filefern",
        name: "Filefern",
        family: CardArtFamily::Bloom,
        bias: BiasProfile::Researcher,
    },
    SpeciesSeed {
        id: "fluxbud",
        name: "Fluxbud",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "glintseed",
        name: "Glintseed",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "graphbud",
        name: "Graphbud",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "hushbloom",
        name: "Hushbloom",
        family: CardArtFamily::Bloom,
        bias: BiasProfile::Researcher,
    },
    SpeciesSeed {
        id: "hushpatch",
        name: "Hushpatch",
        family: CardArtFamily::Shell,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "inkdrift",
        name: "Inkdrift",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "jadebud",
        name: "Jadebud",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "junospark",
        name: "Junospark",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "keystep",
        name: "Keystep",
        family: CardArtFamily::Shell,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "linenode",
        name: "Linenode",
        family: CardArtFamily::Shell,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "logleaf",
        name: "Logleaf",
        family: CardArtFamily::Bloom,
        bias: BiasProfile::Researcher,
    },
    SpeciesSeed {
        id: "lumenpatch",
        name: "Lumenpatch",
        family: CardArtFamily::Shell,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "mintbyte",
        name: "Mintbyte",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "mosslatch",
        name: "Mosslatch",
        family: CardArtFamily::Shell,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "notchbud",
        name: "Notchbud",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "patchfern",
        name: "Patchfern",
        family: CardArtFamily::Shell,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "pathpetal",
        name: "Pathpetal",
        family: CardArtFamily::Bloom,
        bias: BiasProfile::Researcher,
    },
    SpeciesSeed {
        id: "pixmote",
        name: "Pixmote",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "promptbud",
        name: "Promptbud",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "quillbud",
        name: "Quillbud",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "rillspark",
        name: "Rillspark",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "softseed",
        name: "Softseed",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "stackbud",
        name: "Stackbud",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "tracefern",
        name: "Tracefern",
        family: CardArtFamily::Bloom,
        bias: BiasProfile::Researcher,
    },
    SpeciesSeed {
        id: "velobud",
        name: "Velobud",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "warmglyph",
        name: "Warmglyph",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "whispforge",
        name: "Whispforge",
        family: CardArtFamily::Shell,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "wiremote",
        name: "Wiremote",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "yardrift",
        name: "Yardrift",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "yonbud",
        name: "Yonbud",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "zenpatch",
        name: "Zenpatch",
        family: CardArtFamily::Shell,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "zipling",
        name: "Zipling",
        family: CardArtFamily::Bud,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "zirruspark",
        name: "Zirruspark",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "duskbloom",
        name: "Duskbloom",
        family: CardArtFamily::Bloom,
        bias: BiasProfile::Researcher,
    },
    SpeciesSeed {
        id: "pebblewisp",
        name: "Pebblewisp",
        family: CardArtFamily::Spark,
        bias: BiasProfile::BuilderOperator,
    },
];

const RARE_SPECIES: &[SpeciesSeed] = &[
    SpeciesSeed {
        id: "aetherquill",
        name: "Aetherquill",
        family: CardArtFamily::Shade,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "aetherprism",
        name: "Aetherprism",
        family: CardArtFamily::Prism,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "aetherweaver",
        name: "Aetherweaver",
        family: CardArtFamily::Weaver,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "velvetshade",
        name: "Velvetshade",
        family: CardArtFamily::Shade,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "velvetcrest",
        name: "Velvetcrest",
        family: CardArtFamily::Crest,
        bias: BiasProfile::ReviewerOperator,
    },
    SpeciesSeed {
        id: "velvetpetal",
        name: "Velvetpetal",
        family: CardArtFamily::Shade,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "cinderweaver",
        name: "Cinderweaver",
        family: CardArtFamily::Weaver,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "cindercrest",
        name: "Cindercrest",
        family: CardArtFamily::Crest,
        bias: BiasProfile::ReviewerOperator,
    },
    SpeciesSeed {
        id: "cindershade",
        name: "Cindershade",
        family: CardArtFamily::Shade,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "haloprism",
        name: "Haloprism",
        family: CardArtFamily::Prism,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "halocrest",
        name: "Halocrest",
        family: CardArtFamily::Crest,
        bias: BiasProfile::ReviewerOperator,
    },
    SpeciesSeed {
        id: "haloshade",
        name: "Haloshade",
        family: CardArtFamily::Shade,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "quartzquill",
        name: "Quartzquill",
        family: CardArtFamily::Shade,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "quartzpetal",
        name: "Quartzpetal",
        family: CardArtFamily::Shade,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "quartzweaver",
        name: "Quartzweaver",
        family: CardArtFamily::Weaver,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "auroracrest",
        name: "Auroracrest",
        family: CardArtFamily::Crest,
        bias: BiasProfile::ReviewerOperator,
    },
    SpeciesSeed {
        id: "brambleprism",
        name: "Brambleprism",
        family: CardArtFamily::Prism,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "celestshade",
        name: "Celestshade",
        family: CardArtFamily::Shade,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "dawnweaver",
        name: "Dawnweaver",
        family: CardArtFamily::Weaver,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "emberquill",
        name: "Emberquill",
        family: CardArtFamily::Shade,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "frostpetal",
        name: "Frostpetal",
        family: CardArtFamily::Shade,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "gloamcrest",
        name: "Gloamcrest",
        family: CardArtFamily::Crest,
        bias: BiasProfile::ReviewerOperator,
    },
    SpeciesSeed {
        id: "harborprism",
        name: "Harborprism",
        family: CardArtFamily::Prism,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "ivoryweaver",
        name: "Ivoryweaver",
        family: CardArtFamily::Weaver,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "latticequill",
        name: "Latticequill",
        family: CardArtFamily::Shade,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "moonpetal",
        name: "Moonpetal",
        family: CardArtFamily::Shade,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "opalshade",
        name: "Opalshade",
        family: CardArtFamily::Shade,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "runecrest",
        name: "Runecrest",
        family: CardArtFamily::Crest,
        bias: BiasProfile::ReviewerOperator,
    },
    SpeciesSeed {
        id: "silkprism",
        name: "Silkprism",
        family: CardArtFamily::Prism,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "thornweaver",
        name: "Thornweaver",
        family: CardArtFamily::Weaver,
        bias: BiasProfile::BuilderResearch,
    },
];

const LEGENDARY_SPECIES: &[SpeciesSeed] = &[
    SpeciesSeed {
        id: "legendary-slot-01",
        name: "Legendary Slot 01",
        family: CardArtFamily::Crown,
        bias: BiasProfile::ResearcherOperator,
    },
    SpeciesSeed {
        id: "legendary-slot-02",
        name: "Legendary Slot 02",
        family: CardArtFamily::Crown,
        bias: BiasProfile::ResearcherOperator,
    },
    SpeciesSeed {
        id: "legendary-slot-03",
        name: "Legendary Slot 03",
        family: CardArtFamily::Crown,
        bias: BiasProfile::ResearcherOperator,
    },
    SpeciesSeed {
        id: "legendary-slot-04",
        name: "Legendary Slot 04",
        family: CardArtFamily::Crown,
        bias: BiasProfile::ResearcherOperator,
    },
    SpeciesSeed {
        id: "legendary-slot-05",
        name: "Legendary Slot 05",
        family: CardArtFamily::Crown,
        bias: BiasProfile::ResearcherOperator,
    },
    SpeciesSeed {
        id: "legendary-slot-06",
        name: "Legendary Slot 06",
        family: CardArtFamily::Crown,
        bias: BiasProfile::ResearcherOperator,
    },
    SpeciesSeed {
        id: "legendary-slot-07",
        name: "Legendary Slot 07",
        family: CardArtFamily::Crown,
        bias: BiasProfile::ResearcherOperator,
    },
    SpeciesSeed {
        id: "legendary-slot-08",
        name: "Legendary Slot 08",
        family: CardArtFamily::Crown,
        bias: BiasProfile::ResearcherOperator,
    },
    SpeciesSeed {
        id: "legendary-slot-09",
        name: "Legendary Slot 09",
        family: CardArtFamily::Crown,
        bias: BiasProfile::ResearcherOperator,
    },
    SpeciesSeed {
        id: "legendary-slot-10",
        name: "Legendary Slot 10",
        family: CardArtFamily::Crown,
        bias: BiasProfile::ResearcherOperator,
    },
];

#[derive(Clone, Debug)]
pub(crate) struct VivlingSpeciesDefinition {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) rarity: VivlingRarity,
    pub(crate) availability: VivlingAvailability,
    pub(crate) ascii_baby: String,
    pub(crate) ascii_juvenile: String,
    pub(crate) ascii_adult: String,
    pub(crate) bias: WorkAffinitySet,
    pub(crate) card_baby: VivlingStageCardArt,
    pub(crate) card_juvenile: VivlingStageCardArt,
    pub(crate) card_adult: VivlingStageCardArt,
    footer_family: CardArtFamily,
}

#[derive(Clone, Debug)]
pub(crate) struct VivlingCardArt {
    pub(crate) lines: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct VivlingStageCardArt {
    pub(crate) narrow_lines: Vec<String>,
    pub(crate) wide_lines: Vec<String>,
}

pub(crate) fn species_registry() -> &'static [VivlingSpeciesDefinition] {
    static REGISTRY: OnceLock<Vec<VivlingSpeciesDefinition>> = OnceLock::new();
    REGISTRY.get_or_init(build_species_registry).as_slice()
}

pub(crate) fn active_species_registry() -> Vec<&'static VivlingSpeciesDefinition> {
    species_registry()
        .iter()
        .filter(|species| species.availability.is_user_visible())
        .collect()
}

pub(crate) fn hatch_species(hash: u64) -> &'static VivlingSpeciesDefinition {
    let active = active_species_registry();
    let idx = (hash as usize) % active.len().max(1);
    active.get(idx).copied().unwrap_or(&species_registry()[0])
}

pub(crate) fn species_for_id(id: &str) -> &'static VivlingSpeciesDefinition {
    species_registry()
        .iter()
        .find(|species| species.id == id)
        .unwrap_or(&species_registry()[0])
}

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

fn build_species_registry() -> Vec<VivlingSpeciesDefinition> {
    let mut definitions = Vec::with_capacity(100);
    for seed in COMMON_SPECIES {
        definitions.push(build_species(seed, VivlingRarity::Common));
    }
    for seed in RARE_SPECIES {
        definitions.push(build_species(seed, VivlingRarity::Rare));
    }
    for seed in LEGENDARY_SPECIES {
        definitions.push(build_species(seed, VivlingRarity::Legendary));
    }
    definitions
}

fn build_species(seed: &SpeciesSeed, rarity: VivlingRarity) -> VivlingSpeciesDefinition {
    let card_baby = stage_card_art(seed, rarity, Stage::Baby);
    let card_juvenile = stage_card_art(seed, rarity, Stage::Juvenile);
    let card_adult = stage_card_art(seed, rarity, Stage::Adult);
    let availability = match rarity {
        VivlingRarity::Legendary => VivlingAvailability::Reserved,
        VivlingRarity::Common | VivlingRarity::Rare => VivlingAvailability::Active,
    };
    VivlingSpeciesDefinition {
        id: seed.id.to_string(),
        name: seed.name.to_string(),
        rarity,
        availability,
        ascii_baby: footer_sprite_for_species(seed, rarity, Stage::Baby),
        ascii_juvenile: footer_sprite_for_species(seed, rarity, Stage::Juvenile),
        ascii_adult: footer_sprite_for_species(seed, rarity, Stage::Adult),
        bias: bias_for_profile(seed.bias, rarity),
        card_baby,
        card_juvenile,
        card_adult,
        footer_family: seed.family,
    }
}

fn footer_sprite_for_species(seed: &SpeciesSeed, rarity: VivlingRarity, stage: Stage) -> String {
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
    let idx = species_variant(id, name, stage) % 6;
    match rarity {
        VivlingRarity::Common => COMMON_MARKERS[idx],
        VivlingRarity::Rare => RARE_MARKERS[idx],
        VivlingRarity::Legendary => LEGENDARY_MARKERS[idx],
    }
}

fn stage_card_art(seed: &SpeciesSeed, rarity: VivlingRarity, stage: Stage) -> VivlingStageCardArt {
    let narrow = card_art_lines(seed.family, stage, true);
    let wide = card_art_lines(seed.family, stage, false);
    VivlingStageCardArt {
        narrow_lines: personalize_card_lines(seed, rarity, stage, narrow),
        wide_lines: personalize_card_lines(seed, rarity, stage, wide),
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

fn species_variant(id: &str, name: &str, stage: Stage) -> usize {
    let stage_salt = match stage {
        Stage::Baby => 11usize,
        Stage::Juvenile => 23usize,
        Stage::Adult => 37usize,
    };
    id.bytes()
        .chain(name.bytes())
        .fold(stage_salt, |acc, byte| {
            acc.wrapping_mul(33).wrapping_add(byte as usize)
        })
}

fn variant_symbol(variant: usize) -> char {
    ['^', '*', '~', '+', 'o', '#', '%', '='][variant % 8]
}

fn eye_symbol(variant: usize) -> char {
    ['.', '\'', ':', '*', 'o', ';', '`', '+'][variant % 8]
}

fn rarity_badge(rarity: VivlingRarity, variant: usize) -> char {
    let common = ['.', '\'', '~', ','];
    let rare = ['*', '+', '^', '!'];
    let legendary = ['#', '@', '$', '%'];
    match rarity {
        VivlingRarity::Common => common[variant % 4],
        VivlingRarity::Rare => rare[variant % 4],
        VivlingRarity::Legendary => legendary[variant % 4],
    }
}

fn species_mark(id: &str, name: &str) -> String {
    let mut id_letters = id.chars().filter(|ch| ch.is_ascii_alphabetic());
    let first = id_letters.next().unwrap_or('x');
    let second = id_letters.next().unwrap_or(first);
    let third = name
        .chars()
        .rev()
        .find(|ch| ch.is_ascii_alphabetic())
        .unwrap_or('x');
    format!(
        "{}{}{}",
        first.to_ascii_uppercase(),
        second.to_ascii_uppercase(),
        third.to_ascii_uppercase()
    )
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

fn bias_for_profile(profile: BiasProfile, rarity: VivlingRarity) -> WorkAffinitySet {
    let scale = match rarity {
        VivlingRarity::Common => 1,
        VivlingRarity::Rare => 2,
        VivlingRarity::Legendary => 3,
    };
    let pair = |a: u64, b: u64, c: u64, d: u64| WorkAffinitySet {
        builder: a * scale,
        reviewer: b * scale,
        researcher: c * scale,
        operator: d * scale,
    };
    match profile {
        BiasProfile::BuilderResearch => pair(12, 2, 8, 4),
        BiasProfile::BuilderOperator => pair(12, 2, 4, 8),
        BiasProfile::Researcher => pair(4, 2, 14, 2),
        BiasProfile::ReviewerResearch => pair(2, 12, 8, 4),
        BiasProfile::ReviewerOperator => pair(2, 12, 4, 8),
        BiasProfile::ResearcherOperator => pair(3, 4, 10, 10),
    }
}
