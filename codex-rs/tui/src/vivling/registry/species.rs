use super::super::model::{Stage, WorkAffinitySet};
use super::card_art::stage_card_art;
use super::footer::footer_sprite_for_species;
use super::types::{
    BiasProfile, CardArtFamily, SpeciesSeed, VivlingAvailability, VivlingRarity,
    VivlingSpeciesDefinition,
};

const COMMON_SPECIES: &[SpeciesSeed] = &[
    SpeciesSeed {
        id: "syllo",
        name: "Syllo",
        family: CardArtFamily::Weaver,
        bias: BiasProfile::BuilderResearch,
    },
    SpeciesSeed {
        id: "tracebit",
        name: "Tracebit",
        family: CardArtFamily::Spark,
        bias: BiasProfile::ReviewerOperator,
    },
    SpeciesSeed {
        id: "memoirn",
        name: "Memoirn",
        family: CardArtFamily::Bud,
        bias: BiasProfile::Researcher,
    },
    SpeciesSeed {
        id: "checklet",
        name: "Checklet",
        family: CardArtFamily::Shell,
        bias: BiasProfile::ReviewerOperator,
    },
    SpeciesSeed {
        id: "forgekin",
        name: "Forgekin",
        family: CardArtFamily::Crest,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "seekmote",
        name: "Seekmote",
        family: CardArtFamily::Bloom,
        bias: BiasProfile::Researcher,
    },
];

const RARE_SPECIES: &[SpeciesSeed] = &[
    SpeciesSeed {
        id: "orchestra",
        name: "Orchestra",
        family: CardArtFamily::Weaver,
        bias: BiasProfile::ResearcherOperator,
    },
    SpeciesSeed {
        id: "vaultshade",
        name: "Vaultshade",
        family: CardArtFamily::Crest,
        bias: BiasProfile::ReviewerOperator,
    },
    SpeciesSeed {
        id: "dataprism",
        name: "Dataprism",
        family: CardArtFamily::Prism,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "musequill",
        name: "Musequill",
        family: CardArtFamily::Shade,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "logwarden",
        name: "Logwarden",
        family: CardArtFamily::Shell,
        bias: BiasProfile::ReviewerOperator,
    },
];

const LEGENDARY_SPECIES: &[SpeciesSeed] = &[
    SpeciesSeed {
        id: "chronosworn",
        name: "Chronosworn",
        family: CardArtFamily::Crown,
        bias: BiasProfile::ResearcherOperator,
    },
    SpeciesSeed {
        id: "reasonveil",
        name: "Reasonveil",
        family: CardArtFamily::Prism,
        bias: BiasProfile::ReviewerResearch,
    },
    SpeciesSeed {
        id: "releaseforge",
        name: "Releaseforge",
        family: CardArtFamily::Crest,
        bias: BiasProfile::BuilderOperator,
    },
    SpeciesSeed {
        id: "graphoracle",
        name: "Graphoracle",
        family: CardArtFamily::Bloom,
        bias: BiasProfile::Researcher,
    },
];

const MYTHIC_SPECIES: &[SpeciesSeed] = &[SpeciesSeed {
    id: "zed",
    name: "ZED",
    family: CardArtFamily::Crown,
    bias: BiasProfile::ResearcherOperator,
}];

pub(super) fn build_species_registry() -> Vec<VivlingSpeciesDefinition> {
    let mut definitions = Vec::with_capacity(16);
    for seed in COMMON_SPECIES {
        definitions.push(build_species(seed, VivlingRarity::Common));
    }
    for seed in RARE_SPECIES {
        definitions.push(build_species(seed, VivlingRarity::Rare));
    }
    for seed in LEGENDARY_SPECIES {
        definitions.push(build_species(seed, VivlingRarity::Legendary));
    }
    for seed in MYTHIC_SPECIES {
        definitions.push(build_species(seed, VivlingRarity::Mythic));
    }
    definitions
}

fn build_species(seed: &SpeciesSeed, rarity: VivlingRarity) -> VivlingSpeciesDefinition {
    let card_baby = stage_card_art(seed, rarity, Stage::Baby);
    let card_juvenile = stage_card_art(seed, rarity, Stage::Juvenile);
    let card_adult = stage_card_art(seed, rarity, Stage::Adult);
    let availability = match rarity {
        VivlingRarity::Legendary | VivlingRarity::Mythic => VivlingAvailability::Reserved,
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

fn bias_for_profile(profile: BiasProfile, rarity: VivlingRarity) -> WorkAffinitySet {
    let scale = match rarity {
        VivlingRarity::Common => 1,
        VivlingRarity::Rare => 2,
        VivlingRarity::Legendary => 3,
        VivlingRarity::Mythic => 4,
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
