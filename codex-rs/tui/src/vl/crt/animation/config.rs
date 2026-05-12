//! Vivling CRT animation config.
//!
//! Read independently from `~/.codex/config.toml` under `[vivling.crt]`.
//! Kept out of the upstream `ConfigToml` to minimise merge surface: upstream
//! parsing silently ignores unknown sections, so we can co-exist without
//! touching the shared schema.

use std::path::Path;

use serde::Deserialize;

const CONFIG_FILENAME: &str = "config.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VivlingCrtConfig {
    pub scanlines: bool,
    pub phosphor_glow: bool,
    pub flicker: bool,
    pub transitions: bool,
    pub boot_animation: bool,
    pub idle_microanim: bool,
}

impl Default for VivlingCrtConfig {
    fn default() -> Self {
        Self {
            scanlines: true,
            phosphor_glow: true,
            flicker: true,
            transitions: true,
            boot_animation: true,
            idle_microanim: true,
        }
    }
}

impl VivlingCrtConfig {
    /// Load `[vivling.crt]` from `<codex_home>/config.toml`.
    /// Missing file or missing section returns defaults.
    pub(crate) fn load_from_codex_home(codex_home: &Path) -> Self {
        let path = codex_home.join(CONFIG_FILENAME);
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return Self::default(),
        };
        Self::from_toml_str(&raw).unwrap_or_default()
    }

    pub(crate) fn from_toml_str(raw: &str) -> Option<Self> {
        let envelope: ConfigEnvelope = toml::from_str(raw).ok()?;
        let crt = envelope.vivling?.crt?;
        let defaults = Self::default();
        Some(Self {
            scanlines: crt.scanlines.unwrap_or(defaults.scanlines),
            phosphor_glow: crt.phosphor_glow.unwrap_or(defaults.phosphor_glow),
            flicker: crt.flicker.unwrap_or(defaults.flicker),
            transitions: crt.transitions.unwrap_or(defaults.transitions),
            boot_animation: crt.boot_animation.unwrap_or(defaults.boot_animation),
            idle_microanim: crt.idle_microanim.unwrap_or(defaults.idle_microanim),
        })
    }

    /// Convenience: at least one stateful animation effect is enabled.
    pub(crate) fn any_animation_active(&self) -> bool {
        self.flicker || self.transitions || self.boot_animation || self.idle_microanim
    }
}

#[derive(Debug, Deserialize, Default)]
struct ConfigEnvelope {
    #[serde(default)]
    vivling: Option<VivlingTable>,
}

#[derive(Debug, Deserialize, Default)]
struct VivlingTable {
    #[serde(default)]
    crt: Option<CrtTable>,
}

#[derive(Debug, Deserialize, Default)]
struct CrtTable {
    scanlines: Option<bool>,
    phosphor_glow: Option<bool>,
    flicker: Option<bool>,
    transitions: Option<bool>,
    boot_animation: Option<bool>,
    idle_microanim: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_all_on() {
        let c = VivlingCrtConfig::default();
        assert!(c.scanlines);
        assert!(c.phosphor_glow);
        assert!(c.flicker);
        assert!(c.transitions);
        assert!(c.boot_animation);
        assert!(c.idle_microanim);
    }

    #[test]
    fn missing_section_yields_defaults() {
        let raw = "model = \"gpt-5\"\n[tui]\nshow = true\n";
        let parsed = VivlingCrtConfig::from_toml_str(raw);
        assert!(parsed.is_none());
    }

    #[test]
    fn empty_section_yields_defaults_filled() {
        let raw = "[vivling.crt]\n";
        let c = VivlingCrtConfig::from_toml_str(raw).unwrap();
        assert_eq!(c, VivlingCrtConfig::default());
    }

    #[test]
    fn partial_overrides_are_respected() {
        let raw = "[vivling.crt]\nflicker = false\nboot_animation = false\n";
        let c = VivlingCrtConfig::from_toml_str(raw).unwrap();
        assert!(c.scanlines);
        assert!(c.phosphor_glow);
        assert!(!c.flicker);
        assert!(c.transitions);
        assert!(!c.boot_animation);
        assert!(c.idle_microanim);
    }

    #[test]
    fn invalid_toml_falls_back() {
        let raw = "[[[ this is not toml";
        assert!(VivlingCrtConfig::from_toml_str(raw).is_none());
    }

    #[test]
    fn any_animation_active_reflects_individual_toggles() {
        let mut c = VivlingCrtConfig::default();
        assert!(c.any_animation_active());
        c.flicker = false;
        c.transitions = false;
        c.boot_animation = false;
        c.idle_microanim = false;
        assert!(!c.any_animation_active());
    }
}
