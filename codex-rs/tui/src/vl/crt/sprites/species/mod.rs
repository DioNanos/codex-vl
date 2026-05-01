mod chronosworn;
mod orchestra;
mod syllo;
mod zed;

use super::super::director::CrtMode;
use super::super::frame::Frame;
use crate::vivling::Stage;

pub(crate) fn art_for(species_id: &str, stage: Stage, mode: CrtMode, tick: u64) -> Option<Frame> {
    match species_id {
        "syllo" => Some(syllo::art_for(stage, mode, tick)),
        "orchestra" => Some(orchestra::art_for(stage, mode, tick)),
        "chronosworn" => Some(chronosworn::art_for(stage, mode, tick)),
        "zed" => Some(zed::art_for(stage, mode, tick)),
        _ => None,
    }
}
