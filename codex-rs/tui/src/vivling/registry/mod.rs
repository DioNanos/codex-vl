mod card_art;
mod footer;
mod hatch;
mod species;
mod symbols;
mod types;

use std::sync::OnceLock;

pub(crate) use card_art::card_art_for_species;
pub(crate) use footer::active_footer_sprites_for_species;
pub(crate) use hatch::active_species_registry;
#[cfg(test)]
pub(crate) use hatch::hatch_species;
pub(crate) use hatch::hatch_species_from_unlocked;
pub(crate) use hatch::species_for_id;
#[cfg(test)]
pub(crate) use types::VivlingAvailability;
#[cfg(test)]
pub(crate) use types::VivlingRarity;
pub(crate) use types::VivlingSpeciesDefinition;

pub(crate) fn species_registry() -> &'static [VivlingSpeciesDefinition] {
    static REGISTRY: OnceLock<Vec<VivlingSpeciesDefinition>> = OnceLock::new();
    REGISTRY
        .get_or_init(species::build_species_registry)
        .as_slice()
}

#[cfg(test)]
mod tests {
    use super::super::model::Stage;
    use super::*;

    fn card_text(id: &str, stage: Stage) -> String {
        let species = species_for_id(id);
        card_art_for_species(species, stage, 80).lines.join("\n")
    }

    #[test]
    fn primary_species_use_manual_card_art() {
        let syllo = card_text("syllo", Stage::Adult);
        let orchestra = card_text("orchestra", Stage::Adult);
        let chronosworn = card_text("chronosworn", Stage::Adult);

        assert!(syllo.contains("\\.-o-./"));
        assert!(syllo.contains("/___/_<_\\___\\"));
        assert!(orchestra.contains("o    .-o-.    o"));
        assert!(orchestra.contains("/|||\\"));
        assert!(chronosworn.contains("o-.-o"));
        assert!(chronosworn.contains("_|O|_"));
        assert_ne!(syllo, orchestra);
        assert_ne!(orchestra, chronosworn);
    }

    #[test]
    fn manual_card_art_is_ascii_and_bounded() {
        for id in ["syllo", "orchestra", "chronosworn"] {
            for stage in [Stage::Baby, Stage::Juvenile, Stage::Adult] {
                let species = species_for_id(id);
                let card = card_art_for_species(species, stage, 80);
                assert!(
                    card.lines.iter().all(|line| line.is_ascii()),
                    "{id} {stage:?} card must stay ASCII"
                );
                assert!(
                    card.lines.iter().all(|line| line.len() <= 32),
                    "{id} {stage:?} card must stay compact: {:?}",
                    card.lines
                );
            }
        }
    }

    #[test]
    fn zed_stays_on_archive_card_path() {
        let zed_runtime_card = card_text("zed", Stage::Adult);

        assert!(!zed_runtime_card.contains("ZED THE PRIME"));
        assert!(!zed_runtime_card.contains("'-._.-'"));
    }
}
