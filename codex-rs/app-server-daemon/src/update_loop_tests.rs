use pretty_assertions::assert_eq;

use super::install_latest_standalone;
use super::reexec_managed_updater;
use super::update_modes_for_identities;
use crate::RestartMode;
use crate::UpdaterRefreshMode;
use crate::managed_install::executable_identity_from_bytes;
use codex_install_context::InstallContext;

#[test]
fn unchanged_updater_uses_version_based_restart() {
    assert_eq!(
        update_modes_for_identities(
            &executable_identity_from_bytes(b"same"),
            &executable_identity_from_bytes(b"same"),
        ),
        (RestartMode::IfVersionChanged, UpdaterRefreshMode::None)
    );
}

#[test]
fn changed_updater_forces_refresh_even_when_version_may_match() {
    assert_eq!(
        update_modes_for_identities(
            &executable_identity_from_bytes(b"old"),
            &executable_identity_from_bytes(b"new"),
        ),
        (
            RestartMode::Always,
            UpdaterRefreshMode::ReexecIfManagedBinaryChanged,
        )
    );
}

/// codex-vl fork (F-bis): the standalone auto-updater MUST be
/// disabled. The fork must never fetch the upstream openai/codex
/// install script, because executing it would replace the fork
/// binary with unrelated upstream codex and silently strip every
/// codex-vl feature.
#[tokio::test]
async fn install_latest_standalone_is_disabled_in_fork() {
    let result = install_latest_standalone().await;
    let err = result.expect_err(
        "codex-vl fork: install_latest_standalone must return an error so \
         the updater loop never executes the upstream installer",
    );
    let message = format!("{err}");
    assert!(
        message.contains("codex-vl fork"),
        "fork-disabled error must identify the fork. Was: {message}",
    );
    assert!(
        message.contains("disabled"),
        "fork-disabled error must state the updater is disabled. Was: {message}",
    );
    assert!(
        message.contains("@mmmbuto/codex-vl"),
        "fork-disabled error must point users at the fork's npm package. \
         Was: {message}",
    );
}

#[test]
fn reexec_managed_updater_short_circuits_for_npm_and_bun() {
    let missing = std::path::Path::new("/definitely/not/a/codex-vl-binary");

    reexec_managed_updater(missing, &InstallContext::Npm).expect("npm short-circuits");
    reexec_managed_updater(missing, &InstallContext::Bun).expect("bun short-circuits");
}
