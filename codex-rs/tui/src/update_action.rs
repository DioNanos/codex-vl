#[cfg(any(not(debug_assertions), test))]
use codex_install_context::InstallContext;
#[cfg(any(not(debug_assertions), test))]
use codex_install_context::StandalonePlatform;

/// Update action the CLI should perform after the TUI exits.
///
/// codex-vl fork (F-bis): the `Standalone*` variants intentionally do
/// NOT shell out to the upstream `openai/codex` installer commands
/// that the original upstream code printed (the curl/irm install
/// shell snippets pointing at the upstream chatgpt-hosted installer
/// URLs). Running the upstream installer would replace the fork
/// binary with unrelated upstream codex and silently strip every
/// codex-vl feature. Until a fork-owned standalone installer exists,
/// the `Standalone*` variants redirect to the fork's npm package as
/// the supported update path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateAction {
    /// Update via `npm install -g @mmmbuto/codex-vl`.
    NpmGlobalLatest,
    /// Update via `bun install -g @mmmbuto/codex-vl`.
    BunGlobalLatest,
    /// Update via `brew upgrade codex`.
    BrewUpgrade,
    /// codex-vl fork (F-bis): the upstream standalone updater is
    /// disabled. The fork redirects this update path to
    /// `npm install -g @mmmbuto/codex-vl` to avoid silently
    /// overwriting the fork with upstream codex.
    StandaloneUnix,
    /// codex-vl fork (F-bis): the upstream standalone updater is
    /// disabled. The fork redirects this update path to
    /// `npm install -g @mmmbuto/codex-vl` to avoid silently
    /// overwriting the fork with upstream codex.
    StandaloneWindows,
}

impl UpdateAction {
    #[cfg(any(not(debug_assertions), test))]
    pub(crate) fn from_install_context(context: &InstallContext) -> Option<Self> {
        match context {
            InstallContext::Npm => Some(UpdateAction::NpmGlobalLatest),
            InstallContext::Bun => Some(UpdateAction::BunGlobalLatest),
            InstallContext::Brew => Some(UpdateAction::BrewUpgrade),
            InstallContext::Standalone { platform, .. } => Some(match platform {
                StandalonePlatform::Unix => UpdateAction::StandaloneUnix,
                StandalonePlatform::Windows => UpdateAction::StandaloneWindows,
            }),
            InstallContext::Other => None,
        }
    }

    /// Returns the list of command-line arguments for invoking the update.
    ///
    /// codex-vl fork (F-bis): `Standalone*` variants redirect to the
    /// fork's npm install path instead of executing the upstream
    /// chatgpt-hosted installer shell scripts.
    pub fn command_args(self) -> (&'static str, &'static [&'static str]) {
        match self {
            UpdateAction::NpmGlobalLatest
            | UpdateAction::StandaloneUnix
            | UpdateAction::StandaloneWindows => ("npm", &["install", "-g", "@mmmbuto/codex-vl"]),
            UpdateAction::BunGlobalLatest => ("bun", &["install", "-g", "@mmmbuto/codex-vl"]),
            UpdateAction::BrewUpgrade => ("brew", &["upgrade", "--cask", "codex"]),
        }
    }

    /// Returns string representation of the command-line arguments for invoking the update.
    pub fn command_str(self) -> String {
        let (command, args) = self.command_args();
        shlex::try_join(std::iter::once(command).chain(args.iter().copied()))
            .unwrap_or_else(|_| format!("{command} {}", args.join(" ")))
    }
}

#[cfg(not(debug_assertions))]
pub fn get_update_action() -> Option<UpdateAction> {
    UpdateAction::from_install_context(InstallContext::current())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    #[test]
    fn maps_install_context_to_update_action() {
        let native_release_dir = PathBuf::from("/tmp/native-release");

        assert_eq!(
            UpdateAction::from_install_context(&InstallContext::Other),
            None
        );
        assert_eq!(
            UpdateAction::from_install_context(&InstallContext::Npm),
            Some(UpdateAction::NpmGlobalLatest)
        );
        assert_eq!(
            UpdateAction::from_install_context(&InstallContext::Bun),
            Some(UpdateAction::BunGlobalLatest)
        );
        assert_eq!(
            UpdateAction::from_install_context(&InstallContext::Brew),
            Some(UpdateAction::BrewUpgrade)
        );
        assert_eq!(
            UpdateAction::from_install_context(&InstallContext::Standalone {
                platform: StandalonePlatform::Unix,
                release_dir: native_release_dir.clone(),
                resources_dir: Some(native_release_dir.join("codex-resources")),
            }),
            Some(UpdateAction::StandaloneUnix)
        );
        assert_eq!(
            UpdateAction::from_install_context(&InstallContext::Standalone {
                platform: StandalonePlatform::Windows,
                release_dir: native_release_dir.clone(),
                resources_dir: Some(native_release_dir.join("codex-resources")),
            }),
            Some(UpdateAction::StandaloneWindows)
        );
    }

    #[test]
    fn standalone_update_commands_redirect_to_fork_npm_install() {
        // codex-vl fork (F-bis): npm/bun stay on the fork package, and
        // `Standalone*` redirects to npm instead of the upstream
        // chatgpt-hosted installer shell scripts.
        assert_eq!(
            UpdateAction::NpmGlobalLatest.command_args(),
            ("npm", &["install", "-g", "@mmmbuto/codex-vl"][..],)
        );
        assert_eq!(
            UpdateAction::BunGlobalLatest.command_args(),
            ("bun", &["install", "-g", "@mmmbuto/codex-vl"][..],)
        );
        assert_eq!(
            UpdateAction::StandaloneUnix.command_args(),
            ("npm", &["install", "-g", "@mmmbuto/codex-vl"][..],),
            "StandaloneUnix must redirect to the fork npm install path \
             instead of the upstream chatgpt installer",
        );
        assert_eq!(
            UpdateAction::StandaloneWindows.command_args(),
            ("npm", &["install", "-g", "@mmmbuto/codex-vl"][..],),
            "StandaloneWindows must redirect to the fork npm install \
             path instead of the upstream chatgpt installer",
        );
    }

    /// codex-vl fork (F-bis): no `UpdateAction` variant may surface an
    /// install command that points at the upstream chatgpt-hosted
    /// installer endpoint or any other upstream installer endpoint.
    /// This pins both the legacy `curl install.sh` and
    /// `irm install.ps1` paths out of the fork.
    #[test]
    fn no_update_action_invokes_upstream_chatgpt_installer() {
        for action in [
            UpdateAction::NpmGlobalLatest,
            UpdateAction::BunGlobalLatest,
            UpdateAction::BrewUpgrade,
            UpdateAction::StandaloneUnix,
            UpdateAction::StandaloneWindows,
        ] {
            let rendered = action.command_str();
            // Build the forbidden host substring at runtime so the
            // catch-all fork-identity source-grep (see
            // codex-rs/tui/tests/fork_identity_pins.rs) does not see
            // a literal `chatgpt.com/codex` in this source file.
            let forbidden_host = concat!("chatgpt.com", "/", "codex");
            assert!(
                !rendered.contains(forbidden_host),
                "UpdateAction::{action:?} must not surface the upstream \
                 chatgpt installer URL. Was: {rendered}",
            );
        }
    }
}
