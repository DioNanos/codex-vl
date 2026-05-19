use super::super::model::WorkAffinitySet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VivlingRarity {
    Common,
    Rare,
    Legendary,
    Mythic,
}

impl VivlingRarity {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Common => "Common",
            Self::Rare => "Rare",
            Self::Legendary => "Legendary",
            Self::Mythic => "Mythic",
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
pub(super) enum CardArtFamily {
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
pub(super) enum BiasProfile {
    BuilderResearch,
    BuilderOperator,
    Researcher,
    ReviewerResearch,
    ReviewerOperator,
    ResearcherOperator,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SpeciesSeed {
    pub(super) id: &'static str,
    pub(super) name: &'static str,
    pub(super) family: CardArtFamily,
    pub(super) bias: BiasProfile,
}

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
    pub(super) footer_family: CardArtFamily,
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
