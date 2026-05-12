//! Per-species boot sprites used by the Rich-tier boot animation.
//!
//! Each sprite is an 8-row art frame (`BOOT_SPRITE_HEIGHT`) with two
//! eye states (closed during the warm-up phase, open after the blink).
//! Width is bounded by `BOOT_SPRITE_WIDTH` so callers can centre the
//! frame without runtime measurement.

mod chronosworn;
mod orchestra;
mod syllo;
mod zed;

pub(crate) const BOOT_SPRITE_HEIGHT: u16 = 8;
pub(crate) const BOOT_SPRITE_WIDTH: u16 = 17;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BootEyeState {
    Closed,
    Open,
}

#[derive(Clone, Copy)]
pub(crate) struct BootSprite {
    pub rows_closed: [&'static str; 8],
    pub rows_open: [&'static str; 8],
    pub greeting: &'static str,
}

impl BootSprite {
    pub(crate) fn rows(&self, eyes: BootEyeState) -> &[&'static str; 8] {
        match eyes {
            BootEyeState::Closed => &self.rows_closed,
            BootEyeState::Open => &self.rows_open,
        }
    }
}

/// Returns a boot sprite for the species, falling back to the syllo
/// sprite when the id is unknown.
pub(crate) fn boot_sprite_for_species(species_id: &str) -> &'static BootSprite {
    match species_id {
        "syllo" => &syllo::BOOT,
        "zed" => &zed::BOOT,
        "orchestra" => &orchestra::BOOT,
        "chronosworn" => &chronosworn::BOOT,
        _ => &syllo::BOOT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_sprite(s: &BootSprite) {
        assert_eq!(s.rows_closed.len(), BOOT_SPRITE_HEIGHT as usize);
        assert_eq!(s.rows_open.len(), BOOT_SPRITE_HEIGHT as usize);
        for r in s.rows_closed.iter().chain(s.rows_open.iter()) {
            assert!(
                r.chars().count() <= BOOT_SPRITE_WIDTH as usize,
                "row exceeds BOOT_SPRITE_WIDTH ({}): {:?}",
                BOOT_SPRITE_WIDTH,
                r,
            );
        }
        assert!(!s.greeting.is_empty());
        assert!(s.greeting.chars().count() <= 24);
    }

    #[test]
    fn all_species_sprites_respect_geometry() {
        for id in ["syllo", "zed", "orchestra", "chronosworn"] {
            check_sprite(boot_sprite_for_species(id));
        }
    }

    #[test]
    fn unknown_species_falls_back_to_syllo() {
        let unknown = boot_sprite_for_species("missing-id");
        let syllo = boot_sprite_for_species("syllo");
        assert_eq!(unknown.greeting, syllo.greeting);
        assert_eq!(unknown.rows_open[0], syllo.rows_open[0]);
    }

    #[test]
    fn closed_and_open_differ_on_eye_row() {
        for id in ["syllo", "zed", "orchestra", "chronosworn"] {
            let s = boot_sprite_for_species(id);
            assert_ne!(
                s.rows_closed, s.rows_open,
                "{id} closed/open must differ"
            );
        }
    }
}
