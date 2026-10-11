//! Vivling CRT animation config.
//!
//! Read independently from `~/.codex/config.toml` under `[vivling.crt]`.
//! Kept out of the upstream `ConfigToml` to minimise merge surface: upstream
//! parsing silently ignores unknown sections, so we can co-exist without
//! touching the shared schema.

use crate::legacy_core::config::edit::ConfigEdit;
use crate::legacy_core::config::edit::apply_blocking;
use std::path::Path;

use serde::Deserialize;
use toml_edit::DocumentMut;

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

    /// Set `[vivling] layout` in `<codex_home>/config.toml`.
    ///
    /// The edit is delegated to the shared structured TOML editor behind
    /// model, effort and theme persistence (`legacy_core::config::edit`), so
    /// comments, key order and every other entry survive the edit and the
    /// write is atomic. The write is fail-closed: an unparsable file, or a
    /// `vivling` entry that is not a table, is never overwritten. After a
    /// successful write the file is re-parsed to confirm the layout really
    /// round-tripped.
    pub(crate) fn save_layout_to_codex_home(
        codex_home: &Path,
        layout: Self,
    ) -> std::io::Result<()> {
        let path = codex_home.join(CONFIG_FILENAME);
        validate_layout_target(&path)?;
        let edit = ConfigEdit::SetPath {
            segments: vec!["vivling".to_string(), "layout".to_string()],
            value: toml_edit::value(layout.as_config_value().to_string()),
        };
        apply_blocking(codex_home, &[edit]).map_err(|err| {
            std::io::Error::other(format!("failed to persist the Vivling layout: {err:#}"))
        })?;
        verify_saved_layout(&path, layout)
    }
}

/// Reject edits that the shared persistence engine could not apply
/// faithfully: an unparsable `config.toml`, or a `vivling` entry that is not
/// a table (nor an inline table), must never be overwritten. The engine
/// would replace such a value with a fresh table, so the target shape is
/// validated before anything is written.
fn validate_layout_target(path: &Path) -> std::io::Result<()> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    let document = raw.parse::<DocumentMut>().map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("config.toml is not valid TOML: {err}"),
        )
    })?;
    match document.get("vivling") {
        Some(item) if item.is_table_like() => Ok(()),
        Some(_) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "config.toml declares `vivling` but it is not a table; refusing to overwrite it",
        )),
        None => Ok(()),
    }
}

/// Re-read the persisted file and confirm the round-trip through the same
/// `toml` parse the loader uses: the saved document must expose exactly the
/// layout that was requested.
fn verify_saved_layout(path: &Path, layout: VivlingLayout) -> std::io::Result<()> {
    let raw = std::fs::read_to_string(path)?;
    let envelope: ConfigEnvelope = toml::from_str(&raw).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("config.toml is no longer valid TOML after saving the layout: {err}"),
        )
    })?;
    let saved = envelope.vivling.and_then(|table| table.layout);
    if saved.as_deref() == Some(layout.as_config_value()) {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "config.toml does not contain vivling layout \"{}\" after saving it",
                layout.as_config_value()
            ),
        ))
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
    use tempfile::tempdir;

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
    fn save_layout_persists_all_oracle_inputs() {
        // The nine auditor-oracle inputs: every one of them must save to a
        // valid TOML document whose vivling.layout reads back as "line".
        let inputs = [
            "model = \"demo\"\n[vivling]\nlayout = \"full\"\n",
            "model = \"demo\"\n[vivling]\nother = 1\n",
            "model = \"demo\"\n[vivling] # layout preferences\nlayout = \"full\"\n",
            "[vivling]  \nlayout = \"full\"\n",
            "vivling = { layout = \"full\" }\n",
            "[vivling]\nother=1\n[other] # preserve table scope\nx=2\n",
            "[vivling]\n\"layout\" = \"full\"\n",
            "model = \"demo\"\n",
            "[vivling]\nother=1\n[vivling.crt]\nflicker=false\n",
        ];
        for (index, raw) in inputs.into_iter().enumerate() {
            let home = tempdir().expect("tmpdir");
            write_config(home.path(), raw);
            VivlingLayout::save_layout_to_codex_home(home.path(), VivlingLayout::Line)
                .unwrap_or_else(|err| panic!("oracle case {index} ({raw:?}) failed: {err}"));
            assert_eq!(
                saved_layout(home.path()).as_deref(),
                Some("line"),
                "oracle case {index} ({raw:?}) did not round-trip"
            );
        }
    }

    #[test]
    fn save_layout_preserves_comments_and_other_keys() {
        let home = tempdir().expect("tmpdir");
        let raw = concat!(
            "# top comment\n",
            "model = \"gpt-5\"\n",
            "\n",
            "[vivling.crt]\n",
            "flicker = false # keep CRT\n",
            "\n",
            "[vivling]\n",
            "layout = \"full\" # user note\n",
            "other = 1\n",
        );
        write_config(home.path(), raw);
        VivlingLayout::save_layout_to_codex_home(home.path(), VivlingLayout::Line)
            .expect("save layout");
        let updated = read_config(home.path());
        assert!(updated.contains("# top comment"));
        assert!(updated.contains("model = \"gpt-5\""));
        assert!(updated.contains("[vivling.crt]"));
        assert!(updated.contains("flicker = false # keep CRT"));
        // The replaced line keeps its trailing comment and drops the old value.
        assert!(updated.contains("layout = \"line\" # user note"));
        assert!(updated.contains("other = 1"));
        assert!(!updated.contains("layout = \"full\""));
        assert_eq!(updated.matches("[vivling").count(), 2);
        assert_eq!(saved_layout(home.path()).as_deref(), Some("line"));
    }

    #[test]
    fn save_layout_never_duplicates_the_vivling_table() {
        // Regression for the line-writer bug: headers carrying comments or
        // trailing whitespace were missed, so a second [vivling] table was
        // appended and the resulting file was invalid TOML.
        let inputs = [
            "model = \"demo\"\n[vivling]\nother = 1\n",
            "model = \"demo\"\n[vivling] # layout preferences\nlayout = \"full\"\n",
            "[vivling]  \nlayout = \"full\"\n",
        ];
        for raw in inputs {
            let home = tempdir().expect("tmpdir");
            write_config(home.path(), raw);
            VivlingLayout::save_layout_to_codex_home(home.path(), VivlingLayout::Line)
                .unwrap_or_else(|err| panic!("input {raw:?} failed: {err}"));
            let updated = read_config(home.path());
            assert_eq!(updated.matches("[vivling]").count(), 1, "input: {raw:?}");
            assert_eq!(saved_layout(home.path()).as_deref(), Some("line"));
        }
    }

    #[test]
    fn save_layout_keeps_the_key_out_of_child_tables() {
        let home = tempdir().expect("tmpdir");
        write_config(
            home.path(),
            "[vivling]\nother=1\n[vivling.crt]\nflicker=false\n",
        );
        VivlingLayout::save_layout_to_codex_home(home.path(), VivlingLayout::Line)
            .expect("save layout");
        let updated = read_config(home.path());
        let parsed: toml::Table = toml::from_str(&updated).expect("valid TOML");
        let vivling = parsed.get("vivling").expect("vivling table");
        assert_eq!(
            vivling.get("layout").and_then(toml::Value::as_str),
            Some("line")
        );
        assert_eq!(
            vivling.get("other").and_then(toml::Value::as_integer),
            Some(1)
        );
        let crt = vivling.get("crt").expect("vivling.crt table");
        assert_eq!(
            crt.get("flicker").and_then(toml::Value::as_bool),
            Some(false)
        );
        assert!(crt.get("layout").is_none());
        // The plain key must be rendered before the sub-table header: a key
        // placed after [vivling.crt] would belong to the child table.
        let layout_pos = updated.find("layout = \"line\"").expect("layout line");
        let crt_pos = updated.find("[vivling.crt]").expect("crt header");
        assert!(layout_pos < crt_pos);
    }

    #[test]
    fn save_layout_rejects_non_table_vivling_and_keeps_the_file_intact() {
        let home = tempdir().expect("tmpdir");
        let raw = "model = \"demo\"\nvivling = 3\n";
        write_config(home.path(), raw);
        let err = VivlingLayout::save_layout_to_codex_home(home.path(), VivlingLayout::Line)
            .expect_err("a non-table vivling entry must be rejected");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(read_config(home.path()), raw);
    }

    #[test]
    fn save_layout_rejects_unparsable_config_and_keeps_the_file_intact() {
        let home = tempdir().expect("tmpdir");
        let raw = "[[[ this is not toml";
        write_config(home.path(), raw);
        let err = VivlingLayout::save_layout_to_codex_home(home.path(), VivlingLayout::Line)
            .expect_err("an unparsable config must be rejected");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(read_config(home.path()), raw);
    }

    #[test]
    fn save_layout_creates_a_missing_file() {
        let home = tempdir().expect("tmpdir");
        VivlingLayout::save_layout_to_codex_home(home.path(), VivlingLayout::Line)
            .expect("save layout without an existing config");
        assert_eq!(saved_layout(home.path()).as_deref(), Some("line"));
    }

    #[test]
    fn save_layout_accepts_an_empty_file() {
        let home = tempdir().expect("tmpdir");
        write_config(home.path(), "");
        VivlingLayout::save_layout_to_codex_home(home.path(), VivlingLayout::Line)
            .expect("save layout on an empty config");
        assert_eq!(saved_layout(home.path()).as_deref(), Some("line"));
        assert!(read_config(home.path()).contains("[vivling]"));
    }

    #[test]
    fn save_layout_is_idempotent() {
        let home = tempdir().expect("tmpdir");
        write_config(
            home.path(),
            "# top\nmodel = \"gpt-5\"\n[vivling]\nlayout = \"full\" # note\n",
        );
        VivlingLayout::save_layout_to_codex_home(home.path(), VivlingLayout::Line)
            .expect("first save");
        let first = read_config(home.path());
        VivlingLayout::save_layout_to_codex_home(home.path(), VivlingLayout::Line)
            .expect("second save");
        let second = read_config(home.path());
        assert_eq!(first, second);
    }

    #[test]
    fn save_layout_converts_an_inline_vivling_table() {
        let home = tempdir().expect("tmpdir");
        write_config(
            home.path(),
            "model = \"demo\"\nvivling = { layout = \"full\", other = 2 }\n",
        );
        VivlingLayout::save_layout_to_codex_home(home.path(), VivlingLayout::Line)
            .expect("save layout on an inline table");
        let parsed: toml::Table = toml::from_str(&read_config(home.path())).expect("valid TOML");
        let vivling = parsed.get("vivling").expect("vivling table");
        assert_eq!(
            vivling.get("layout").and_then(toml::Value::as_str),
            Some("line")
        );
        assert_eq!(
            vivling.get("other").and_then(toml::Value::as_integer),
            Some(2)
        );
    }

    fn write_config(home: &Path, raw: &str) {
        std::fs::write(home.join(CONFIG_FILENAME), raw).expect("write config.toml");
    }

    fn read_config(home: &Path) -> String {
        std::fs::read_to_string(home.join(CONFIG_FILENAME)).expect("read config.toml")
    }

    /// Parse the saved file with the serde `toml` crate (the same parser the
    /// loader uses) and return the persisted `vivling.layout` value.
    fn saved_layout(home: &Path) -> Option<String> {
        let parsed: toml::Table = toml::from_str(&read_config(home)).expect("valid TOML");
        parsed
            .get("vivling")
            .and_then(|vivling| vivling.get("layout"))
            .and_then(toml::Value::as_str)
            .map(str::to_string)
    }
}
