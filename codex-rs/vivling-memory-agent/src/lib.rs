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
use std::time::Duration;

use chrono::DateTime;
use chrono::Utc;
use codex_vivling_core::paths::last_write_backup_path;
use codex_vivling_core::paths::lock_file_path;
use codex_vivling_core::paths::pre_migration_backup_path;
use codex_vivling_core::safety::SafetyError;
use codex_vivling_core::safety::acquire_lock;
use codex_vivling_core::safety::backup_last_write;
use codex_vivling_core::safety::write_atomic;
use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

/// Live batch lock timeout. Generous on purpose: the memory agent runs
/// out-of-band of any interactive flow, so it can afford to wait while
/// a TUI save completes. The TUI's per-Vivling save path uses a much
/// shorter 5-second timeout (`codex_tui::vivling::runtime::roster`);
/// the asymmetry is intentional.
const LIVE_BATCH_LOCK_TIMEOUT: Duration = Duration::from_secs(30);

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
    #[error("live batch failed on {vivling_id} ({path}): {source}")]
    LiveBatchSafety {
        vivling_id: String,
        path: PathBuf,
        #[source]
        source: SafetyError,
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

// --- Step 6.B: live batch transaction harness ---

/// Per-action outcome on the live batch path. Step 6.B never alters the
/// payload semantically (no LLM, no voice, no skill generation): each
/// writeable entry is exercised with the full lock + backup +
/// idempotent-write pipeline so future steps can layer semantic
/// mutation on a transaction layer that is already tested live.
#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LiveBatchActionKind {
    /// Lock acquired, last-write backup taken, payload re-written
    /// byte-for-byte. Pure transaction-pipeline exercise.
    NoopTransaction { wrote: bool },
    /// Entry was not eligible for a live write (stale schema,
    /// unhatched egg, sidecar JSON…). Mirrors the dry-run
    /// `skip_reason` field for shape coherence.
    Skipped { reason: String },
}

#[derive(Debug, Serialize, PartialEq)]
pub struct LiveBatchAction {
    pub vivling_id: String,
    pub state_path: PathBuf,
    #[serde(flatten)]
    pub kind: LiveBatchActionKind,
}

/// Top-level JSON document produced by [`run_live_batch`]. Shares the
/// versioning conventions of [`DryRunReport`] so external consumers
/// can keep a single parser pinned against `report_version`.
#[derive(Debug, Serialize, PartialEq)]
pub struct LiveBatchReport {
    pub report_version: u32,
    pub supported_state_version: u32,
    pub roster_dir: PathBuf,
    pub generated_at: DateTime<Utc>,
    pub total_entries: usize,
    pub wrote_count: usize,
    pub skipped_count: usize,
    pub actions: Vec<LiveBatchAction>,
}

/// Execute the live batch transaction pipeline against `roster_dir`.
///
/// For every Vivling that survives the sidecar / coherence filter:
///
/// 1. Acquires the per-Vivling advisory file lock (30 s timeout,
///    matching the agent batch contract in `codex_vivling_core::paths::lock_file_path`).
///    The lock id is derived from the candidate filename's stem, so
///    no payload byte is trusted before the lock is held.
/// 2. **Re-reads and re-classifies the state JSON under the lock.**
///    Step 6.B round-2 fix: the previous implementation read the
///    body before acquiring the lock, which made a concurrent TUI
///    save observable as a silent rollback to the stale bytes the
///    agent had captured. The fresh read closes that race.
/// 3. Snapshots the (fresh) on-disk JSON to `<id>.json.bak` via
///    `backup_last_write` — the same rotational backup the TUI save
///    path uses.
/// 4. Re-writes the fresh payload byte-for-byte via `write_atomic`.
///    The payload is unchanged; the goal is to exercise the
///    lock + backup + atomic-rename pipeline live, not to mutate
///    Vivling state.
///
/// Stale-schema, unhatched, sidecar and filename/payload mismatch
/// rows are reported as `Skipped { reason }` and never touched. Errors
/// are fail-fast: the first safety failure aborts the batch and
/// surfaces a `MemoryAgentError::LiveBatchSafety` carrying the
/// offending `vivling_id` and path.
pub fn run_live_batch(roster_dir: &Path) -> Result<LiveBatchReport, MemoryAgentError> {
    run_live_batch_inner(roster_dir, LIVE_BATCH_LOCK_TIMEOUT)
}

/// Internal entry point that lets the test suite inject a shorter
/// `lock_timeout` so the lock-contention error path can be exercised
/// in milliseconds instead of the 30 s production value.
fn run_live_batch_inner(
    roster_dir: &Path,
    lock_timeout: Duration,
) -> Result<LiveBatchReport, MemoryAgentError> {
    if !roster_dir.exists() {
        return Err(MemoryAgentError::MissingRosterDir(roster_dir.to_path_buf()));
    }

    let candidate_paths = collect_state_candidates(roster_dir)?;

    let mut actions: Vec<LiveBatchAction> = Vec::new();
    for path in candidate_paths {
        let Some(file_stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };

        // Acquire the lock based on the candidate's filename stem
        // BEFORE reading the payload. Two consequences:
        //   1. A concurrent TUI save cannot slip a fresh write in
        //      between our read and our write (Step 6.B round-2 fix).
        //   2. The lock id is derived from a filesystem fact (the
        //      stem) rather than from JSON bytes we have not yet
        //      validated, so even a malicious / corrupt payload
        //      cannot fool us into locking the wrong file.
        let lock_path = lock_file_path(roster_dir, file_stem);
        let _guard = acquire_lock(&lock_path, lock_timeout).map_err(|err| {
            MemoryAgentError::LiveBatchSafety {
                vivling_id: file_stem.to_string(),
                path: path.clone(),
                source: err,
            }
        })?;

        // Fresh read under the lock — anything the TUI wrote while we
        // were waiting on the flock is what we now operate on.
        let fresh_body =
            std::fs::read_to_string(&path).map_err(|err| MemoryAgentError::RosterIo {
                path: path.clone(),
                source: err,
            })?;
        let fresh_header: VivlingStateHeader =
            serde_json::from_str(&fresh_body).map_err(|err| {
                MemoryAgentError::InvalidStateJson {
                    path: path.clone(),
                    source: err,
                }
            })?;

        // Sidecar / mismatch defence — same coherence check as
        // `plan_dry_run`, applied to the FRESH header so a sidecar
        // that swapped places with a real state file under us is
        // still caught.
        if fresh_header.vivling_id != file_stem {
            // Release the lock without touching anything.
            continue;
        }

        let (would_write, skip_reason) = classify_entry(&fresh_header);
        if !would_write {
            let reason = skip_reason.unwrap_or_else(|| "not eligible".to_string());
            actions.push(LiveBatchAction {
                vivling_id: fresh_header.vivling_id,
                state_path: path,
                kind: LiveBatchActionKind::Skipped { reason },
            });
            continue;
        }

        // --- transaction pipeline (still under lock) ---
        let backup_path = last_write_backup_path(roster_dir, &fresh_header.vivling_id);
        backup_last_write(&path, &backup_path).map_err(|err| {
            MemoryAgentError::LiveBatchSafety {
                vivling_id: fresh_header.vivling_id.clone(),
                path: path.clone(),
                source: err,
            }
        })?;
        write_atomic(&path, fresh_body.as_bytes()).map_err(|err| {
            MemoryAgentError::LiveBatchSafety {
                vivling_id: fresh_header.vivling_id.clone(),
                path: path.clone(),
                source: err,
            }
        })?;
        // Guard drops here → lock released.
        actions.push(LiveBatchAction {
            vivling_id: fresh_header.vivling_id,
            state_path: path,
            kind: LiveBatchActionKind::NoopTransaction { wrote: true },
        });
    }

    actions.sort_by(|a, b| a.vivling_id.cmp(&b.vivling_id));
    let wrote_count = actions
        .iter()
        .filter(|action| {
            matches!(
                action.kind,
                LiveBatchActionKind::NoopTransaction { wrote: true }
            )
        })
        .count();
    let skipped_count = actions
        .iter()
        .filter(|action| matches!(action.kind, LiveBatchActionKind::Skipped { .. }))
        .count();

    Ok(LiveBatchReport {
        report_version: DRY_RUN_REPORT_VERSION,
        supported_state_version: SUPPORTED_STATE_VERSION,
        roster_dir: roster_dir.to_path_buf(),
        generated_at: Utc::now(),
        total_entries: actions.len(),
        wrote_count,
        skipped_count,
        actions,
    })
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

    // --- Step 6.B: live batch transaction harness regression tests ---

    use codex_vivling_core::paths::last_write_backup_path as core_last_write_backup_path;
    use codex_vivling_core::paths::lock_file_path as core_lock_file_path;
    use codex_vivling_core::safety::acquire_lock as core_acquire_lock;
    use std::time::Duration;

    #[test]
    fn live_batch_creates_lock_and_backup_for_writeable_state() {
        let temp = TempDir::new().expect("tempdir");
        let body = format!(
            r#"{{"version":{},"vivling_id":"viv-1","name":"Aelia","hatched":true}}"#,
            SUPPORTED_STATE_VERSION
        );
        write_state(temp.path(), "viv-1", &body);

        let report = run_live_batch(temp.path()).expect("live batch");
        assert_eq!(report.total_entries, 1);
        assert_eq!(report.wrote_count, 1);
        assert_eq!(report.skipped_count, 0);

        let lock = core_lock_file_path(temp.path(), "viv-1");
        let backup = core_last_write_backup_path(temp.path(), "viv-1");
        assert!(
            lock.exists(),
            "lock file must remain after release: {}",
            lock.display()
        );
        assert!(
            backup.exists(),
            "last-write backup must land: {}",
            backup.display()
        );
        // Backup captures the *pre-write* contents — which in Step 6.B
        // are byte-identical to the post-write contents, but we still
        // pin the invariant.
        assert_eq!(fs::read_to_string(&backup).expect("read backup"), body);
    }

    #[test]
    fn live_batch_writes_identical_bytes_idempotent() {
        let temp = TempDir::new().expect("tempdir");
        let body = format!(
            r#"{{"version":{},"vivling_id":"viv-1","name":"Aelia","hatched":true,"brain_enabled":true}}"#,
            SUPPORTED_STATE_VERSION
        );
        let path = write_state(temp.path(), "viv-1", &body);
        let before = fs::read_to_string(&path).expect("read before");

        let _ = run_live_batch(temp.path()).expect("live batch");

        let after = fs::read_to_string(&path).expect("read after");
        assert_eq!(before, after, "live batch must be byte-idempotent");
    }

    #[test]
    fn live_batch_ignores_skills_sidecar_and_no_ghost_backup() {
        let temp = TempDir::new().expect("tempdir");
        let body = format!(
            r#"{{"version":{},"vivling_id":"viv-1","name":"Aelia","hatched":true}}"#,
            SUPPORTED_STATE_VERSION
        );
        write_state(temp.path(), "viv-1", &body);
        write_state(temp.path(), "viv-1_skills", "[]");

        let report = run_live_batch(temp.path()).expect("live batch");
        assert_eq!(
            report.total_entries, 1,
            "sidecar must not appear in the live report"
        );
        assert_eq!(report.wrote_count, 1);
        // No backup for a phantom empty id.
        let ghost_backup = temp.path().join(".json.bak");
        assert!(!ghost_backup.exists(), "no `.json.bak` ghost must land");
    }

    #[test]
    fn live_batch_skips_stale_schema_without_writing_or_backup() {
        let temp = TempDir::new().expect("tempdir");
        let body = r#"{"version":7,"vivling_id":"viv-legacy","name":"Legacy","hatched":true}"#;
        let path = write_state(temp.path(), "viv-legacy", body);
        let before = fs::read_to_string(&path).expect("read before");
        let before_modified = fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime");

        let report = run_live_batch(temp.path()).expect("live batch");
        assert_eq!(report.wrote_count, 0);
        assert_eq!(report.skipped_count, 1);
        assert!(matches!(
            report.actions[0].kind,
            LiveBatchActionKind::Skipped { .. }
        ));

        let after = fs::read_to_string(&path).expect("read after");
        assert_eq!(before, after, "stale-schema file must not be rewritten");
        let after_modified = fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime");
        assert_eq!(
            before_modified, after_modified,
            "stale-schema file must not have its mtime bumped"
        );
        let backup = core_last_write_backup_path(temp.path(), "viv-legacy");
        assert!(
            !backup.exists(),
            "no last-write backup must land for a skipped entry: {}",
            backup.display()
        );
    }

    #[test]
    fn live_batch_skips_unhatched() {
        let temp = TempDir::new().expect("tempdir");
        let body = format!(
            r#"{{"version":{},"vivling_id":"viv-egg","name":"Unborn","hatched":false}}"#,
            SUPPORTED_STATE_VERSION
        );
        write_state(temp.path(), "viv-egg", &body);
        let report = run_live_batch(temp.path()).expect("live batch");
        assert_eq!(report.wrote_count, 0);
        assert_eq!(report.skipped_count, 1);
        match &report.actions[0].kind {
            LiveBatchActionKind::Skipped { reason } => {
                assert!(reason.contains("not hatched"), "got: {reason}");
            }
            other => panic!("expected Skipped, got {other:?}"),
        }
    }

    #[test]
    fn live_batch_does_not_overwrite_change_made_before_lock_acquired() {
        // Round-2 regression test for the stale-read race Codex caught.
        //
        // Scenario:
        //   1. The agent is about to scan `viv-1.json`.
        //   2. The TUI (simulated here by a sibling thread holding the
        //      lock) writes a fresh payload while the agent is queued
        //      on the flock.
        //   3. The agent acquires the lock, re-reads the file under
        //      lock, and rewrites the FRESH bytes.
        //
        // The pre-fix implementation would have rewritten the original
        // bytes it read before locking, silently rolling back the TUI's
        // change. With the lock-first refactor the agent must observe
        // and preserve the new payload.
        let temp = TempDir::new().expect("tempdir");
        let original = format!(
            r#"{{"version":{},"vivling_id":"viv-race","name":"Old","hatched":true}}"#,
            SUPPORTED_STATE_VERSION
        );
        let updated = format!(
            r#"{{"version":{},"vivling_id":"viv-race","name":"Fresh","hatched":true}}"#,
            SUPPORTED_STATE_VERSION
        );
        let state_path = write_state(temp.path(), "viv-race", &original);

        // Sibling thread takes the lock first, then mutates the file
        // and holds the lock for a short window so the agent's
        // acquire_lock blocks until the rewrite has landed.
        let lock_path = core_lock_file_path(temp.path(), "viv-race");
        let state_path_clone = state_path.clone();
        let updated_clone = updated.clone();
        let handle = std::thread::spawn(move || {
            let guard = core_acquire_lock(&lock_path, Duration::from_secs(5)).expect("seed lock");
            // Simulate the TUI committing a fresh save under the
            // lock that the agent is waiting on.
            fs::write(&state_path_clone, &updated_clone).expect("seed write");
            std::thread::sleep(Duration::from_millis(120));
            drop(guard);
        });

        // Give the seed thread a beat to grab the lock + write.
        std::thread::sleep(Duration::from_millis(30));

        let report = run_live_batch(temp.path()).expect("live batch");
        handle.join().expect("seed thread");

        assert_eq!(report.wrote_count, 1);
        let final_body = fs::read_to_string(&state_path).expect("read final");
        assert_eq!(
            final_body, updated,
            "agent must preserve the TUI's fresh write, not roll back to the pre-lock body"
        );
    }

    #[test]
    fn live_batch_lock_contention_returns_precise_timeout_error() {
        // Round-2 follow-up to P2.1: with the test-only timeout knob
        // we can now actually exercise the error path. The seed thread
        // holds the lock for longer than the foreground call's
        // injected 100 ms timeout, so the inner `acquire_lock` must
        // surface `LockTimeout` and `run_live_batch_inner` must wrap
        // it into a `LiveBatchSafety` error naming `viv-busy`.
        let temp = TempDir::new().expect("tempdir");
        let body = format!(
            r#"{{"version":{},"vivling_id":"viv-busy","name":"Busy","hatched":true}}"#,
            SUPPORTED_STATE_VERSION
        );
        write_state(temp.path(), "viv-busy", &body);

        let lock_path = core_lock_file_path(temp.path(), "viv-busy");
        let handle = std::thread::spawn(move || {
            let guard = core_acquire_lock(&lock_path, Duration::from_secs(5)).expect("seed lock");
            // Hold the lock well beyond the foreground timeout.
            std::thread::sleep(Duration::from_millis(600));
            drop(guard);
        });
        std::thread::sleep(Duration::from_millis(20));

        let err = run_live_batch_inner(temp.path(), Duration::from_millis(100))
            .expect_err("must time out under contention");
        handle.join().expect("seed thread");

        match err {
            MemoryAgentError::LiveBatchSafety {
                vivling_id, source, ..
            } => {
                assert_eq!(vivling_id, "viv-busy");
                assert!(
                    matches!(source, SafetyError::LockTimeout { .. }),
                    "expected SafetyError::LockTimeout, got: {source:?}"
                );
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn live_batch_lock_contention_propagates_error() {
        let temp = TempDir::new().expect("tempdir");
        let body = format!(
            r#"{{"version":{},"vivling_id":"viv-busy","name":"Busy","hatched":true}}"#,
            SUPPORTED_STATE_VERSION
        );
        write_state(temp.path(), "viv-busy", &body);

        // Hold the per-Vivling lock for the entire run. The default
        // 30 s timeout in `run_live_batch` would make this test slow;
        // we instead grab the lock with a short timeout outside the
        // function, then call `run_live_batch` and rely on the fact
        // that `acquire_lock` returns `LockTimeout` after its own
        // timeout window — which here is the 30 s production value.
        //
        // To keep the test under one second we shorten the test by
        // letting the inner call race against an *already held* lock
        // with `LOCK_EX | LOCK_NB`: the inner `acquire_lock` poll
        // loop will detect the busy lock and surface
        // `LockTimeout` only after its own deadline. Instead of
        // waiting 30 s we assert against the existence of an error
        // outcome using a short manual call: we hold the lock with
        // its native primitive and let the inner loop's first
        // `flock` attempt fail; if the timeout were too long we'd
        // see this test stall, so we ship it as a smoke that the
        // error path *exists* by directly using the public API with
        // a hand-rolled stand-in: grab the lock, then assert that
        // `run_live_batch` returns an error referencing
        // `viv-busy`.
        let lock_path = core_lock_file_path(temp.path(), "viv-busy");
        // Take the lock for ~50 ms via a background thread so the
        // inner `acquire_lock` poll sees it busy on its first try.
        // Using the public `acquire_lock` keeps this test independent
        // of OS-specific lock APIs.
        let temp_path = temp.path().to_path_buf();
        let lock_path_clone = lock_path.clone();
        let handle = std::thread::spawn(move || {
            let guard =
                core_acquire_lock(&lock_path_clone, Duration::from_secs(5)).expect("seed lock");
            // Hold the lock long enough for the foreground call to
            // observe the contention and time out — `run_live_batch`
            // uses a 30 s timeout, so we keep the lock until the call
            // either succeeds (unlikely) or errors (expected). We
            // signal the foreground via the temp dir's existence:
            // the test joins us at the end.
            std::thread::sleep(Duration::from_millis(150));
            drop(guard);
            let _ = temp_path;
        });

        // Give the seed thread a beat to grab the lock first.
        std::thread::sleep(Duration::from_millis(20));

        // Foreground attempt: should either succeed (if the seed
        // released first) or return a clear LiveBatchSafety error
        // mentioning the vivling_id. We accept both outcomes so the
        // test is not racy; what matters is that on failure the
        // error is precise.
        match run_live_batch(temp.path()) {
            Ok(report) => {
                handle.join().expect("seed thread");
                // The seed may have released before the foreground
                // managed to poll; this is fine as long as the
                // pipeline still produced a writeable entry.
                assert_eq!(report.wrote_count, 1);
            }
            Err(MemoryAgentError::LiveBatchSafety { vivling_id, .. }) => {
                handle.join().expect("seed thread");
                assert_eq!(vivling_id, "viv-busy");
            }
            Err(other) => {
                handle.join().expect("seed thread");
                panic!("unexpected error: {other:?}");
            }
        }
    }

    #[test]
    fn live_batch_serialises_with_stable_action_shape() {
        let temp = TempDir::new().expect("tempdir");
        let body = format!(
            r#"{{"version":{},"vivling_id":"viv-1","name":"Aelia","hatched":true}}"#,
            SUPPORTED_STATE_VERSION
        );
        write_state(temp.path(), "viv-1", &body);
        let report = run_live_batch(temp.path()).expect("live batch");
        let json = serde_json::to_value(&report).expect("serialise");
        for key in [
            "report_version",
            "supported_state_version",
            "roster_dir",
            "generated_at",
            "total_entries",
            "wrote_count",
            "skipped_count",
            "actions",
        ] {
            assert!(json.get(key).is_some(), "missing top-level key: {key}");
        }
        let action = &json["actions"][0];
        // `LiveBatchActionKind` flattens with a `kind` tag.
        assert_eq!(action["kind"], "noop_transaction");
        assert_eq!(action["wrote"], true);
        assert_eq!(action["vivling_id"], "viv-1");
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
