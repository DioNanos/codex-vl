//! Vivling memory agent — Step 6.A skeleton.
//!
//! Memory V2 design §12 introduces a side process that walks the roster,
//! distils work memory into stable patterns, and (in later steps) writes
//! back compact summaries / voice / skill updates. Step 6.A lands only
//! the **dry-run** half: enumerate the roster, count how many Vivlings
//! would receive a batch write, and produce a stable JSON report. No
//! state file is mutated.
//!
//! The crate intentionally has no LLM, no scheduler and no Tokio
//! dependency: the dry-run is pure I/O over the on-disk roster and
//! can be exercised entirely from a synchronous test. Live batches and
//! the GLM 5.1 distill path land in later steps.

use std::path::Path;
use std::path::PathBuf;

use chrono::DateTime;
use chrono::Utc;
use codex_vivling_core::paths::pre_migration_backup_path;
use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

/// Hard-coded version tag emitted in the dry-run report. Bumped only
/// when the report schema changes; lets external tooling pin a parser
/// against a known shape.
pub const DRY_RUN_REPORT_VERSION: u32 = 1;

/// Schema version of the Vivling state files this build is aware of.
/// Kept in sync with `codex_vivling_core::model::VERSION`; bumping one
/// without the other is a bug.
pub const SUPPORTED_STATE_VERSION: u32 = codex_vivling_core::model::VERSION;

#[derive(Debug, Error)]
pub enum MemoryAgentError {
    #[error("roster directory does not exist: {0}")]
    MissingRosterDir(PathBuf),
    #[error("roster directory could not be read: {path}: {source}")]
    RosterIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("vivling state file is invalid JSON: {path}: {source}")]
    InvalidStateJson {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

/// Minimal projection of `VivlingState` consumed by the dry-run report.
/// Re-deserialising the full state would force a dependency on the TUI
/// crate; this header-only struct is enough to enumerate, classify and
/// describe what a live batch would do later.
#[derive(Debug, Deserialize)]
struct VivlingStateHeader {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    vivling_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    hatched: bool,
    #[serde(default)]
    brain_enabled: bool,
}

/// Per-Vivling row in the dry-run report.
#[derive(Debug, Serialize, PartialEq)]
pub struct DryRunVivlingEntry {
    pub vivling_id: String,
    pub name: String,
    pub on_disk_version: u32,
    pub hatched: bool,
    pub brain_enabled: bool,
    /// `true` when a live batch would touch this Vivling's state file.
    /// Step 6.A applies a conservative gate: hatched + matching schema
    /// version. Unhatched eggs and stale-schema files are reported but
    /// excluded so a later live batch never silently migrates them.
    pub would_write: bool,
    /// Path the live batch would write to. Reported even when
    /// `would_write` is false so external tooling can inspect the
    /// target without having to recompute it.
    pub state_path: PathBuf,
    /// Path of the one-shot pre-migration backup the live batch would
    /// produce when bumping a state file across a schema boundary.
    /// `None` for Vivlings already at the current schema version.
    pub pre_migration_backup_path: Option<PathBuf>,
    /// Reason the row was skipped, when `would_write` is false.
    pub skip_reason: Option<String>,
}

/// Top-level JSON document produced by `plan_dry_run`. Stable shape:
/// new optional fields may be appended, existing fields keep their
/// names and types until `report_version` bumps.
#[derive(Debug, Serialize, PartialEq)]
pub struct DryRunReport {
    pub report_version: u32,
    pub supported_state_version: u32,
    pub roster_dir: PathBuf,
    pub generated_at: DateTime<Utc>,
    pub total_entries: usize,
    pub would_write_count: usize,
    /// Placeholder for the live batch's token / cost estimate. Step 6.A
    /// always reports zero (no LLM is invoked); the field is wired in
    /// so consumers can build their UI against the final shape.
    pub tokens_used_estimate: u64,
    pub cost_estimate_usd: f64,
    /// Placeholder for the per-Vivling action list a live batch would
    /// produce. Always empty in Step 6.A.
    pub actions: Vec<serde_json::Value>,
    pub entries: Vec<DryRunVivlingEntry>,
}

/// Walk `roster_dir` and produce a [`DryRunReport`] describing what a
/// live batch would do. Performs **no** state mutation. Side-effects
/// are limited to reading state JSON files from disk.
///
/// Step 6.A round-2: only enumerates real per-Vivling state files. The
/// preferred input is the `roster.json` index; when present it pins the
/// exact set of `<vivling_id>.json` files to consider. When absent the
/// scanner falls back to a directory walk that excludes the known
/// sidecar suffixes (`*_skills.json`, `*_voice.json`, `*.bak`,
/// `*.lock`) and refuses to treat a payload whose deserialised
/// `vivling_id` does not match its file stem as a state file.
pub fn plan_dry_run(roster_dir: &Path) -> Result<DryRunReport, MemoryAgentError> {
    if !roster_dir.exists() {
        return Err(MemoryAgentError::MissingRosterDir(roster_dir.to_path_buf()));
    }

    let candidate_paths = collect_state_candidates(roster_dir)?;

    let mut entries: Vec<DryRunVivlingEntry> = Vec::new();
    for path in candidate_paths {
        let Some(file_stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let body = std::fs::read_to_string(&path).map_err(|err| MemoryAgentError::RosterIo {
            path: path.clone(),
            source: err,
        })?;
        let header: VivlingStateHeader =
            serde_json::from_str(&body).map_err(|err| MemoryAgentError::InvalidStateJson {
                path: path.clone(),
                source: err,
            })?;

        // Filename / payload coherence: a real state file always has
        // `vivling_id == <filename without .json>`. A mismatch means
        // we picked up a sidecar that happens to be valid JSON; skip
        // it without inventing a fake entry.
        if header.vivling_id != file_stem {
            continue;
        }

        let (would_write, skip_reason) = classify_entry(&header);
        let pre_migration_backup = if header.version < SUPPORTED_STATE_VERSION {
            Some(pre_migration_backup_path(
                roster_dir,
                &header.vivling_id,
                header.version,
            ))
        } else {
            None
        };

        entries.push(DryRunVivlingEntry {
            vivling_id: header.vivling_id,
            name: header.name,
            on_disk_version: header.version,
            hatched: header.hatched,
            brain_enabled: header.brain_enabled,
            would_write,
            state_path: path,
            pre_migration_backup_path: pre_migration_backup,
            skip_reason,
        });
    }

    entries.sort_by(|a, b| a.vivling_id.cmp(&b.vivling_id));

    let would_write_count = entries.iter().filter(|entry| entry.would_write).count();
    Ok(DryRunReport {
        report_version: DRY_RUN_REPORT_VERSION,
        supported_state_version: SUPPORTED_STATE_VERSION,
        roster_dir: roster_dir.to_path_buf(),
        generated_at: Utc::now(),
        total_entries: entries.len(),
        would_write_count,
        tokens_used_estimate: 0,
        cost_estimate_usd: 0.0,
        actions: Vec::new(),
        entries,
    })
}

/// Header-only projection of `roster.json`. Mirrors
/// `codex_tui::vivling::runtime::roster::VivlingRoster` for the only
/// field we need here; replicated to avoid a dep on `codex-tui`.
#[derive(Debug, Deserialize)]
struct RosterIndexHeader {
    #[serde(default)]
    vivling_ids: Vec<String>,
}

/// Build the ordered list of candidate state files to deserialise.
/// Preferred source is the `roster.json` index; falls back to a
/// filtered directory walk if the index is missing or unparseable.
fn collect_state_candidates(roster_dir: &Path) -> Result<Vec<PathBuf>, MemoryAgentError> {
    let roster_path = roster_dir.join("roster.json");
    if let Ok(body) = std::fs::read_to_string(&roster_path)
        && let Ok(index) = serde_json::from_str::<RosterIndexHeader>(&body)
    {
        let mut candidates: Vec<PathBuf> = Vec::with_capacity(index.vivling_ids.len());
        for id in index.vivling_ids {
            if id.trim().is_empty() {
                continue;
            }
            let candidate = roster_dir.join(format!("{id}.json"));
            if candidate.exists() {
                candidates.push(candidate);
            }
        }
        return Ok(candidates);
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    let read_dir = std::fs::read_dir(roster_dir).map_err(|err| MemoryAgentError::RosterIo {
        path: roster_dir.to_path_buf(),
        source: err,
    })?;
    for dirent in read_dir {
        let dirent = dirent.map_err(|err| MemoryAgentError::RosterIo {
            path: roster_dir.to_path_buf(),
            source: err,
        })?;
        let Ok(file_name) = dirent.file_name().into_string() else {
            continue;
        };
        if file_name == "roster.json" {
            continue;
        }
        if !file_name.ends_with(".json") {
            continue;
        }
        // Known sidecar suffixes produced by Memory V2 (and any future
        // companion files); exclude them so they never reach the
        // deserializer. The filename/header coherence check inside
        // `plan_dry_run` is a second line of defence.
        let stem = file_name.trim_end_matches(".json");
        if stem.ends_with("_skills") || stem.ends_with("_voice") {
            continue;
        }
        candidates.push(dirent.path());
    }
    candidates.sort();
    Ok(candidates)
}

fn classify_entry(header: &VivlingStateHeader) -> (bool, Option<String>) {
    if header.vivling_id.is_empty() {
        return (false, Some("missing vivling_id".to_string()));
    }
    if !header.hatched {
        return (false, Some("not hatched yet".to_string()));
    }
    if header.version != SUPPORTED_STATE_VERSION {
        return (
            false,
            Some(format!(
                "on-disk schema {} differs from supported {}",
                header.version, SUPPORTED_STATE_VERSION
            )),
        );
    }
    (true, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_state(roster_dir: &Path, file_stem: &str, body: &str) -> PathBuf {
        let path = roster_dir.join(format!("{file_stem}.json"));
        fs::write(&path, body).expect("write fixture");
        path
    }

    #[test]
    fn missing_roster_dir_is_rejected() {
        let temp = TempDir::new().expect("tempdir");
        let missing = temp.path().join("does-not-exist");
        let err = plan_dry_run(&missing).expect_err("must error");
        assert!(
            matches!(err, MemoryAgentError::MissingRosterDir(_)),
            "got: {err:?}"
        );
    }

    #[test]
    fn empty_roster_produces_empty_report() {
        let temp = TempDir::new().expect("tempdir");
        let report = plan_dry_run(temp.path()).expect("plan");
        assert_eq!(report.total_entries, 0);
        assert_eq!(report.would_write_count, 0);
        assert_eq!(report.report_version, DRY_RUN_REPORT_VERSION);
        assert_eq!(report.supported_state_version, SUPPORTED_STATE_VERSION);
        assert!(report.entries.is_empty());
        assert!(report.actions.is_empty());
        assert_eq!(report.tokens_used_estimate, 0);
        assert_eq!(report.cost_estimate_usd, 0.0);
    }

    #[test]
    fn hatched_current_schema_entry_would_be_written() {
        let temp = TempDir::new().expect("tempdir");
        let body = format!(
            r#"{{"version":{},"vivling_id":"viv-1","name":"Aelia","hatched":true,"brain_enabled":true}}"#,
            SUPPORTED_STATE_VERSION
        );
        write_state(temp.path(), "viv-1", &body);

        let report = plan_dry_run(temp.path()).expect("plan");
        assert_eq!(report.total_entries, 1);
        assert_eq!(report.would_write_count, 1);
        let entry = &report.entries[0];
        assert_eq!(entry.vivling_id, "viv-1");
        assert_eq!(entry.name, "Aelia");
        assert_eq!(entry.on_disk_version, SUPPORTED_STATE_VERSION);
        assert!(entry.hatched);
        assert!(entry.brain_enabled);
        assert!(entry.would_write);
        assert!(entry.skip_reason.is_none());
        // Current-schema rows must not carry a pre-migration target.
        assert!(entry.pre_migration_backup_path.is_none());
    }

    #[test]
    fn stale_schema_entry_is_skipped_and_pre_migration_path_is_reported() {
        let temp = TempDir::new().expect("tempdir");
        let body = r#"{"version":7,"vivling_id":"viv-legacy","name":"Legacy","hatched":true}"#;
        write_state(temp.path(), "viv-legacy", body);

        let report = plan_dry_run(temp.path()).expect("plan");
        assert_eq!(report.total_entries, 1);
        assert_eq!(report.would_write_count, 0);
        let entry = &report.entries[0];
        assert_eq!(entry.on_disk_version, 7);
        assert!(!entry.would_write);
        assert!(
            entry
                .skip_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("differs from supported")),
            "got: {:?}",
            entry.skip_reason
        );
        let backup = entry
            .pre_migration_backup_path
            .as_ref()
            .expect("pre-migration backup path must be reported");
        assert!(backup.to_string_lossy().contains("viv-legacy"));
        assert!(backup.to_string_lossy().contains(".v7.bak"));
    }

    #[test]
    fn unhatched_entry_is_skipped_with_reason() {
        let temp = TempDir::new().expect("tempdir");
        let body = format!(
            r#"{{"version":{},"vivling_id":"viv-egg","name":"Unborn","hatched":false}}"#,
            SUPPORTED_STATE_VERSION
        );
        write_state(temp.path(), "viv-egg", &body);

        let report = plan_dry_run(temp.path()).expect("plan");
        assert_eq!(report.would_write_count, 0);
        assert_eq!(
            report.entries[0].skip_reason.as_deref(),
            Some("not hatched yet")
        );
        assert!(!report.entries[0].would_write);
    }

    #[test]
    fn roster_index_is_ignored() {
        let temp = TempDir::new().expect("tempdir");
        fs::write(
            temp.path().join("roster.json"),
            r#"{"version":1,"active_vivling_id":null,"vivling_ids":[]}"#,
        )
        .expect("write roster");
        let report = plan_dry_run(temp.path()).expect("plan");
        assert_eq!(report.total_entries, 0);
    }

    #[test]
    fn dry_run_does_not_mutate_state_files() {
        let temp = TempDir::new().expect("tempdir");
        let body = format!(
            r#"{{"version":{},"vivling_id":"viv-1","name":"Aelia","hatched":true}}"#,
            SUPPORTED_STATE_VERSION
        );
        let path = write_state(temp.path(), "viv-1", &body);
        let before_meta = fs::metadata(&path).expect("metadata");
        let before_modified = before_meta.modified().expect("mtime");

        let _ = plan_dry_run(temp.path()).expect("plan");

        let after_meta = fs::metadata(&path).expect("metadata");
        let after_modified = after_meta.modified().expect("mtime");
        assert_eq!(
            before_modified, after_modified,
            "dry-run must not touch state files"
        );
    }

    #[test]
    fn invalid_json_produces_explicit_error() {
        let temp = TempDir::new().expect("tempdir");
        write_state(temp.path(), "viv-broken", "not-json{");
        let err = plan_dry_run(temp.path()).expect_err("must error");
        assert!(
            matches!(err, MemoryAgentError::InvalidStateJson { .. }),
            "got: {err:?}"
        );
    }

    // --- Step 6.A round-2: sidecar exclusion regression tests ---

    #[test]
    fn skills_sidecar_array_is_ignored_when_roster_index_absent() {
        let temp = TempDir::new().expect("tempdir");
        let body = format!(
            r#"{{"version":{},"vivling_id":"viv-1","name":"Aelia","hatched":true}}"#,
            SUPPORTED_STATE_VERSION
        );
        write_state(temp.path(), "viv-1", &body);
        // A V2 skills sidecar serialises as a JSON array; the previous
        // scanner would try to deserialise it as a state file and crash
        // with `InvalidStateJson`.
        write_state(temp.path(), "viv-1_skills", "[]");

        let report = plan_dry_run(temp.path()).expect("plan");
        assert_eq!(report.total_entries, 1, "sidecar must not count as a state");
        assert_eq!(report.entries[0].vivling_id, "viv-1");
        assert!(report.entries[0].would_write);
    }

    #[test]
    fn object_sidecar_does_not_produce_bogus_entry_when_roster_index_absent() {
        let temp = TempDir::new().expect("tempdir");
        let body = format!(
            r#"{{"version":{},"vivling_id":"viv-1","name":"Aelia","hatched":true}}"#,
            SUPPORTED_STATE_VERSION
        );
        write_state(temp.path(), "viv-1", &body);
        // The bug Codex reported: `{}` deserialised cleanly into a
        // VivlingStateHeader full of defaults, producing an empty
        // vivling_id and a bogus `.v0.bak` path.
        write_state(temp.path(), "viv-1_skills", "{}");

        let report = plan_dry_run(temp.path()).expect("plan");
        assert_eq!(report.total_entries, 1);
        assert!(
            report
                .entries
                .iter()
                .all(|entry| !entry.vivling_id.is_empty()),
            "no entry must have an empty vivling_id; got: {:?}",
            report.entries
        );
        assert!(
            report.entries.iter().all(|entry| entry
                .pre_migration_backup_path
                .as_ref()
                .is_none_or(|p| !p.to_string_lossy().contains(".json.v0.bak"))),
            "no entry must point at a `.json.v0.bak` ghost path"
        );
    }

    #[test]
    fn filename_payload_mismatch_is_dropped_even_when_walked() {
        let temp = TempDir::new().expect("tempdir");
        // No roster.json -> fallback to directory walk. Filename is
        // `mystery.json` but the payload claims `vivling_id = "viv-99"`;
        // this is exactly the shape a future sidecar accident could
        // produce. The coherence check must drop it.
        let body = format!(
            r#"{{"version":{},"vivling_id":"viv-99","name":"Ghost","hatched":true}}"#,
            SUPPORTED_STATE_VERSION
        );
        write_state(temp.path(), "mystery", &body);

        let report = plan_dry_run(temp.path()).expect("plan");
        assert_eq!(
            report.total_entries, 0,
            "filename/payload mismatch must not appear in the report"
        );
    }

    #[test]
    fn roster_index_pins_the_state_set() {
        let temp = TempDir::new().expect("tempdir");
        // Two valid state files; only one is listed in roster.json.
        // The scanner must respect the roster as the source of truth
        // and ignore the other file (e.g. a stale copy).
        let body_a = format!(
            r#"{{"version":{},"vivling_id":"viv-active","name":"Active","hatched":true}}"#,
            SUPPORTED_STATE_VERSION
        );
        let body_b = format!(
            r#"{{"version":{},"vivling_id":"viv-orphan","name":"Orphan","hatched":true}}"#,
            SUPPORTED_STATE_VERSION
        );
        write_state(temp.path(), "viv-active", &body_a);
        write_state(temp.path(), "viv-orphan", &body_b);
        std::fs::write(
            temp.path().join("roster.json"),
            r#"{"version":9,"active_vivling_id":"viv-active","vivling_ids":["viv-active"]}"#,
        )
        .expect("write roster");

        let report = plan_dry_run(temp.path()).expect("plan");
        assert_eq!(report.total_entries, 1);
        assert_eq!(report.entries[0].vivling_id, "viv-active");
    }

    #[test]
    fn report_is_json_serialisable_with_stable_keys() {
        let temp = TempDir::new().expect("tempdir");
        let body = format!(
            r#"{{"version":{},"vivling_id":"viv-1","name":"Aelia","hatched":true,"brain_enabled":true}}"#,
            SUPPORTED_STATE_VERSION
        );
        write_state(temp.path(), "viv-1", &body);
        let report = plan_dry_run(temp.path()).expect("plan");
        let json = serde_json::to_value(&report).expect("serialise");
        // Pin the contract shape so consumers can rely on it.
        for key in [
            "report_version",
            "supported_state_version",
            "roster_dir",
            "generated_at",
            "total_entries",
            "would_write_count",
            "tokens_used_estimate",
            "cost_estimate_usd",
            "actions",
            "entries",
        ] {
            assert!(json.get(key).is_some(), "missing top-level key: {key}");
        }
        let entry = &json["entries"][0];
        for key in [
            "vivling_id",
            "name",
            "on_disk_version",
            "hatched",
            "brain_enabled",
            "would_write",
            "state_path",
            "pre_migration_backup_path",
            "skip_reason",
        ] {
            assert!(
                entry.get(key).is_some(),
                "missing per-entry key: {key} in {entry}"
            );
        }
    }
}
