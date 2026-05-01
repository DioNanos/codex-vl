//! CRT rendering tier seam.
//!
//! Three tiers share the same `CrtSurface`. Detection is env-only and cheap;
//! a width gate forces Safe whenever the strip is too narrow to safely host
//! extended glyphs.

const NARROW_WIDTH_FALLBACK: u16 = 18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CrtTier {
    Safe,
    Rich,
    Image,
}

impl CrtTier {
    pub(crate) fn detect() -> Self {
        Self::from_env(EnvProbe::from_std_env())
    }

    pub(crate) fn from_env(probe: EnvProbe) -> Self {
        if let Some(override_value) = probe.crt_tier_override.as_deref() {
            if let Some(tier) = parse_override(override_value) {
                return tier;
            }
        }
        let term = probe.term.as_deref().unwrap_or("");
        if term.eq_ignore_ascii_case("dumb") {
            return Self::Safe;
        }
        if probe
            .term_program
            .as_deref()
            .is_some_and(|tp| tp.eq_ignore_ascii_case("iTerm.app"))
            || probe.kitty_window_id.is_some()
        {
            return Self::Rich;
        }
        if let Some(colorterm) = probe.colorterm.as_deref() {
            if colorterm.eq_ignore_ascii_case("truecolor")
                || colorterm.eq_ignore_ascii_case("24bit")
            {
                return Self::Rich;
            }
        }
        if term.contains("256color") {
            return Self::Rich;
        }
        Self::Safe
    }

    /// Apply the width gate. Surfaces narrower than `NARROW_WIDTH_FALLBACK`
    /// always render at Safe regardless of the detected tier.
    pub(crate) fn for_width(self, width: u16) -> Self {
        if width < NARROW_WIDTH_FALLBACK {
            Self::Safe
        } else {
            self
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct EnvProbe {
    pub(crate) crt_tier_override: Option<String>,
    pub(crate) term: Option<String>,
    pub(crate) term_program: Option<String>,
    pub(crate) colorterm: Option<String>,
    pub(crate) kitty_window_id: Option<String>,
}

impl EnvProbe {
    pub(crate) fn from_std_env() -> Self {
        Self {
            crt_tier_override: std::env::var("CODEX_VL_CRT_TIER").ok(),
            term: std::env::var("TERM").ok(),
            term_program: std::env::var("TERM_PROGRAM").ok(),
            colorterm: std::env::var("COLORTERM").ok(),
            kitty_window_id: std::env::var("KITTY_WINDOW_ID").ok(),
        }
    }
}

fn parse_override(raw: &str) -> Option<CrtTier> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "safe" => Some(CrtTier::Safe),
        "rich" => Some(CrtTier::Rich),
        "image" => Some(CrtTier::Image),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe() -> EnvProbe {
        EnvProbe::default()
    }

    #[test]
    fn override_safe_wins_over_other_signals() {
        let mut p = probe();
        p.crt_tier_override = Some("safe".into());
        p.term_program = Some("iTerm.app".into());
        assert_eq!(CrtTier::from_env(p), CrtTier::Safe);
    }

    #[test]
    fn override_rich_and_image_are_honoured() {
        let mut p = probe();
        p.crt_tier_override = Some("rich".into());
        assert_eq!(CrtTier::from_env(p), CrtTier::Rich);
        let mut p = probe();
        p.crt_tier_override = Some("image".into());
        assert_eq!(CrtTier::from_env(p), CrtTier::Image);
    }

    #[test]
    fn override_is_case_insensitive_and_trims() {
        let mut p = probe();
        p.crt_tier_override = Some("  RICH  ".into());
        assert_eq!(CrtTier::from_env(p), CrtTier::Rich);
    }

    #[test]
    fn unknown_override_falls_back_to_detection() {
        let mut p = probe();
        p.crt_tier_override = Some("ultra".into());
        p.term = Some("xterm".into());
        assert_eq!(CrtTier::from_env(p), CrtTier::Safe);
    }

    #[test]
    fn term_dumb_forces_safe() {
        let mut p = probe();
        p.term = Some("dumb".into());
        p.colorterm = Some("truecolor".into());
        assert_eq!(CrtTier::from_env(p), CrtTier::Safe);
    }

    #[test]
    fn iterm_program_yields_rich() {
        let mut p = probe();
        p.term_program = Some("iTerm.app".into());
        p.term = Some("xterm".into());
        assert_eq!(CrtTier::from_env(p), CrtTier::Rich);
    }

    #[test]
    fn kitty_window_id_yields_rich() {
        let mut p = probe();
        p.kitty_window_id = Some("17".into());
        p.term = Some("xterm-kitty".into());
        assert_eq!(CrtTier::from_env(p), CrtTier::Rich);
    }

    #[test]
    fn colorterm_truecolor_yields_rich() {
        let mut p = probe();
        p.colorterm = Some("truecolor".into());
        p.term = Some("xterm".into());
        assert_eq!(CrtTier::from_env(p), CrtTier::Rich);
        let mut p = probe();
        p.colorterm = Some("24bit".into());
        assert_eq!(CrtTier::from_env(p), CrtTier::Rich);
    }

    #[test]
    fn term_with_256color_yields_rich() {
        let mut p = probe();
        p.term = Some("screen-256color".into());
        assert_eq!(CrtTier::from_env(p), CrtTier::Rich);
    }

    #[test]
    fn empty_env_falls_back_to_safe() {
        assert_eq!(CrtTier::from_env(probe()), CrtTier::Safe);
    }

    #[test]
    fn width_gate_forces_safe_below_threshold() {
        for w in [0u16, 1, 8, 12, 17] {
            assert_eq!(CrtTier::Rich.for_width(w), CrtTier::Safe);
            assert_eq!(CrtTier::Image.for_width(w), CrtTier::Safe);
            assert_eq!(CrtTier::Safe.for_width(w), CrtTier::Safe);
        }
    }

    #[test]
    fn width_gate_preserves_tier_at_or_above_threshold() {
        for w in [18u16, 24, 40, 80] {
            assert_eq!(CrtTier::Rich.for_width(w), CrtTier::Rich);
            assert_eq!(CrtTier::Image.for_width(w), CrtTier::Image);
            assert_eq!(CrtTier::Safe.for_width(w), CrtTier::Safe);
        }
    }
}
