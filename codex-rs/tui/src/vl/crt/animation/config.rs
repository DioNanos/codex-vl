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
    pub idle_microanim: bool,
}

impl Default for VivlingCrtConfig {
    fn default() -> Self {
        Self {
            scanlines: true,
            phosphor_glow: true,
            flicker: true,
            transitions: true,
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
            idle_microanim: crt.idle_microanim.unwrap_or(defaults.idle_microanim),
        })
    }

    /// Convenience: at least one stateful animation effect is enabled.
    pub(crate) fn any_animation_active(&self) -> bool {
        self.flicker || self.transitions || self.idle_microanim
    }
}

/// Vivling strip layout chosen in `[vivling] layout`.
///
/// `Full` renders the classic three-line CRT strip; `Line` collapses it to a
/// single row (one-line glyph, truncated insight, short mood, slow-blinking
/// dot). Unknown or missing values fall back to `Full`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum VivlingLayout {
    #[default]
    Full,
    Line,
}

impl VivlingLayout {
    /// Parse a user-provided value; anything that is not `full`/`line`
    /// falls back to `Full` (the caller logs the warning once).
    pub(crate) fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "line" => Self::Line,
            _ => Self::Full,
        }
    }

    /// Whether `raw` is a value this type understands (`full` or `line`,
    /// case-insensitive).
    pub(crate) fn is_known_value(raw: &str) -> bool {
        matches!(raw.trim().to_ascii_lowercase().as_str(), "full" | "line")
    }

    pub(crate) fn as_config_value(&self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Line => "line",
        }
    }

    /// Read `[vivling] layout` from `<codex_home>/config.toml`.
    /// Missing file or missing key returns the default (`Full`).
    pub(crate) fn load_from_codex_home(codex_home: &Path) -> Self {
        let raw = match std::fs::read_to_string(codex_home.join(CONFIG_FILENAME)) {
            Ok(s) => s,
            Err(_) => return Self::default(),
        };
        let parsed = toml::from_str::<ConfigEnvelope>(&raw)
            .ok()
            .and_then(|envelope| envelope.vivling)
            .and_then(|table| table.layout);
        match parsed {
            Some(value) if Self::is_known_value(&value) => Self::parse(&value),
            Some(value) => {
                tracing::warn!("unknown vivling layout {value:?}; falling back to \"full\"");
                Self::default()
            }
            None => Self::default(),
        }
    }

    /// Set `[vivling] layout` in `<codex_home>/config.toml` with a minimal
    /// edit: every comment and every other key is preserved byte-for-byte.
    pub(crate) fn save_layout_to_codex_home(
        codex_home: &Path,
        layout: Self,
    ) -> std::io::Result<()> {
        let path = codex_home.join(CONFIG_FILENAME);
        let raw = std::fs::read_to_string(&path).unwrap_or_default();
        std::fs::write(&path, set_layout_in_toml(&raw, layout.as_config_value()))
    }
}

/// Rewrite `[vivling] layout = …` in a config.toml body while preserving
/// every other line byte-for-byte (comments, keys, other tables). When the
/// `[vivling]` table exists but has no `layout` key, the key is inserted
/// right before the next table header (TOML requires plain table keys before
/// any sub-table header such as `[vivling.crt]`). When the table is missing
/// entirely, a new `[vivling]` table is appended.
fn set_layout_in_toml(raw: &str, value: &str) -> String {
    let mut out = String::with_capacity(raw.len() + value.len() + 16);
    let mut in_vivling_table = false;
    let mut written = false;
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_vivling_table && !written {
                out.push_str(&format!("layout = \"{value}\"\n"));
                written = true;
            }
            in_vivling_table = trimmed == "[vivling]";
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if in_vivling_table
            && let Some((key, _)) = trimmed.split('#').next().unwrap_or("").split_once('=')
            && key.trim() == "layout"
        {
            match trimmed.find('#') {
                Some(hash_index) => {
                    let comment = &trimmed[hash_index..];
                    out.push_str(&format!("layout = \"{value}\" {comment}\n"));
                }
                None => out.push_str(&format!("layout = \"{value}\"\n")),
            }
            written = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !written {
        if !out.is_empty() {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
        }
        out.push_str("[vivling]\n");
        out.push_str(&format!("layout = \"{value}\"\n"));
    }
    out
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
    #[serde(default)]
    layout: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct CrtTable {
    scanlines: Option<bool>,
    phosphor_glow: Option<bool>,
    flicker: Option<bool>,
    transitions: Option<bool>,
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
        let raw = "[vivling.crt]\nflicker = false\nidle_microanim = false\n";
        let c = VivlingCrtConfig::from_toml_str(raw).unwrap();
        assert!(c.scanlines);
        assert!(c.phosphor_glow);
        assert!(!c.flicker);
        assert!(c.transitions);
        assert!(!c.idle_microanim);
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
        c.idle_microanim = false;
        assert!(!c.any_animation_active());
    }

    #[test]
    fn layout_defaults_to_full() {
        assert_eq!(VivlingLayout::default(), VivlingLayout::Full);
        let raw = "model = \"gpt-5\"\n";
        let path = std::env::temp_dir().join(format!(
            "vivling-layout-default-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::write(&path, raw).expect("write");
        let layout = VivlingLayout::load_from_codex_home(path.parent().expect("parent"));
        std::fs::remove_file(&path).expect("cleanup");
        assert_eq!(layout, VivlingLayout::Full);
    }

    #[test]
    fn layout_parses_known_values_case_insensitively() {
        assert_eq!(VivlingLayout::parse("line"), VivlingLayout::Line);
        assert_eq!(VivlingLayout::parse(" LINE "), VivlingLayout::Line);
        assert_eq!(VivlingLayout::parse("full"), VivlingLayout::Full);
    }

    #[test]
    fn layout_read_from_config_table() {
        let raw = "[vivling]\nlayout = \"line\"\n";
        let envelope: Result<ConfigEnvelope, _> = toml::from_str(raw);
        assert!(envelope.is_ok());
        assert_eq!(
            VivlingLayout::parse(
                envelope
                    .expect("envelope")
                    .vivling
                    .expect("vivling table")
                    .layout
                    .expect("layout key")
                    .as_str()
            ),
            VivlingLayout::Line
        );
    }

    #[test]
    fn set_layout_preserves_comments_and_other_keys() {
        let raw = "# top comment\nmodel = \"gpt-5\"\n\n[vivling.crt]\nflicker = false # keep CRT\n\n[vivling]\nlayout = \"full\" # user note\nother = 1\n";
        let updated = set_layout_in_toml(raw, "line");
        assert!(updated.contains("# top comment"));
        assert!(updated.contains("model = \"gpt-5\""));
        assert!(updated.contains("[vivling.crt]"));
        assert!(updated.contains("flicker = false # keep CRT"));
        assert!(updated.contains("layout = \"line\" # user note"));
        assert!(updated.contains("other = 1"));
        // The replaced line must keep the comment and drop the old value.
        assert!(!updated.contains("layout = \"full\""));
    }

    #[test]
    fn set_layout_appends_table_when_missing() {
        let raw = "# only upstream keys\nmodel = \"gpt-5\"\n";
        let updated = set_layout_in_toml(raw, "line");
        assert!(updated.starts_with("# only upstream keys"));
        assert!(updated.contains("[vivling]\nlayout = \"line\"\n"));
        assert!(updated.contains("model = \"gpt-5\""));
    }

    #[test]
    fn set_layout_inserts_key_before_sub_table() {
        // A `[vivling.crt]` sub-table header must not capture the plain
        // `layout` key: TOML requires table keys before sub-table headers.
        let raw = "[vivling.crt]\nflicker = true\n";
        let updated = set_layout_in_toml(raw, "line");
        assert!(updated.contains("[vivling]\nlayout = \"line\"\n"));
        assert!(updated.contains("[vivling.crt]\nflicker = true\n"));
    }
}
