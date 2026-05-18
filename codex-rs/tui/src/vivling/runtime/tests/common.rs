pub(super) use super::super::*;
pub(super) use crate::vivling::VivlingLoopEventKind;
pub(super) use crate::vivling::VivlingLoopEventSource;
pub(super) use crate::vivling::model::ADULT_LEVEL;
pub(super) use crate::vivling::model::WORK_XP_PER_LEVEL;
pub(super) use ratatui::buffer::Buffer;
pub(super) use ratatui::layout::Rect;
pub(super) use std::fs::File;
pub(super) use std::io::Write;
pub(super) use tempfile::TempDir;
pub(super) use zip::ZipArchive;
pub(super) use zip::ZipWriter;
pub(super) use zip::write::SimpleFileOptions;

pub(super) fn seeded_state() -> VivlingState {
    VivlingState::new(SeedIdentity {
        value: "install:test-seed".to_string(),
        install_id: Some("test-seed".to_string()),
    })
}

pub(super) fn leveled_state(level: u64, active_days: u64) -> VivlingState {
    let mut state = seeded_state();
    state.active_work_days = active_days;
    state.work_xp = WORK_XP_PER_LEVEL.saturating_mul(level.saturating_sub(1));
    state.recompute_level();
    state
}

pub(super) fn configured_vivling(home: &Path) -> Vivling {
    let mut vivling = Vivling::unavailable();
    vivling.configure(home, AuthCredentialsStoreMode::default());
    vivling.configure_runtime(FrameRequester::test_dummy(), false);
    vivling
}

pub(super) fn hatched_vivling(home: &Path) -> Vivling {
    let mut vivling = configured_vivling(home);
    let _ = vivling
        .command(VivlingAction::Hatch, home)
        .expect("hatch vivling");
    vivling
}

pub(super) fn set_active_level(vivling: &mut Vivling, level: u64) -> VivlingState {
    let mut state = vivling.state.clone().expect("active state");
    state.active_work_days = if level >= ADULT_LEVEL {
        90
    } else if level >= JUVENILE_LEVEL {
        30
    } else {
        level.max(1)
    };
    state.work_xp = WORK_XP_PER_LEVEL.saturating_mul(level.saturating_sub(1));
    state.xp = state.work_xp;
    state.recompute_level();
    vivling.active_vivling_id = Some(state.vivling_id.clone());
    vivling.state = Some(state.clone());
    vivling.save_state().expect("save leveled state");
    state
}

pub(super) fn spawn_ids(vivling: &Vivling, primary_id: &str) -> Vec<String> {
    vivling
        .load_roster()
        .expect("roster")
        .vivling_ids
        .into_iter()
        .filter(|id| id != primary_id)
        .collect()
}

pub(super) fn make_package(path: &Path, manifest: &VivlingPackageManifest, state: &VivlingState) {
    let file = File::create(path).expect("create vivegg");
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file("manifest.json", options)
        .expect("manifest entry");
    zip.write_all(
        serde_json::to_string_pretty(manifest)
            .expect("manifest json")
            .as_bytes(),
    )
    .expect("write manifest");
    zip.start_file("state.json", options).expect("state entry");
    zip.write_all(
        serde_json::to_string_pretty(state)
            .expect("state json")
            .as_bytes(),
    )
    .expect("write state");
    zip.finish().expect("finish package");
}

pub(super) fn write_legacy_state(
    home: &Path,
    state: &VivlingState,
    mutate: impl FnOnce(&mut serde_json::Value),
) {
    let legacy_path = home.join("vivling.json");
    let mut raw = serde_json::to_value(state).expect("serialize legacy state");
    mutate(&mut raw);
    fs::write(
        &legacy_path,
        serde_json::to_string_pretty(&raw).expect("legacy json"),
    )
    .expect("write legacy state");
}

pub(super) fn exportable_state(level: u64) -> VivlingState {
    let mut state = leveled_state(
        level,
        if level >= ADULT_LEVEL {
            90
        } else if level >= JUVENILE_LEVEL {
            30
        } else {
            level.max(1)
        },
    );
    state.primary_vivling_id = state.vivling_id.clone();
    state.is_primary = true;
    state
}
