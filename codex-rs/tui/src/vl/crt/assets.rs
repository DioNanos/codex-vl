use super::director::CrtMode;
use super::frame::Frame;
use super::sprites;
use super::tier::CrtTier;
use crate::vivling::Stage;

pub(crate) fn art_for(
    species_id: &str,
    stage: Stage,
    mode: CrtMode,
    tier: CrtTier,
    tick: u64,
) -> Frame {
    sprites::art_for(species_id, stage, mode, tier, tick)
}
