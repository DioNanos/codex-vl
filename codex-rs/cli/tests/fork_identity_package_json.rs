//! codex-vl fork identity hardening (iter F).
//!
//! Pin the `codex-cli/package.json` manifest so an upstream merge cannot
//! silently retarget the npm package name or repository URL back at the
//! parent `openai/codex` project.

/// Pin: `codex-cli/package.json` keeps its fork-owned identity:
/// `name = "@mmmbuto/codex-vl"` and `repository.url` pointing at the
/// `DioNanos/codex-vl` GitHub repo.
#[test]
fn fork_identity_pin_codex_cli_package_json() {
    const PACKAGE_JSON: &str = include_str!("../../../codex-cli/package.json");
    let manifest: serde_json::Value =
        serde_json::from_str(PACKAGE_JSON).expect("codex-cli/package.json must be valid JSON");

    let name = manifest
        .get("name")
        .and_then(|value| value.as_str())
        .expect("codex-cli/package.json must declare a string `name`");
    assert_eq!(
        name, "@mmmbuto/codex-vl",
        "codex-cli/package.json `name` must remain @mmmbuto/codex-vl",
    );

    let repository_url = manifest
        .get("repository")
        .and_then(|value| value.get("url"))
        .and_then(|value| value.as_str())
        .expect("codex-cli/package.json must declare a string `repository.url`");
    assert!(
        repository_url.contains("DioNanos/codex-vl"),
        "codex-cli/package.json `repository.url` must point at the fork \
         repo (DioNanos/codex-vl). Was: {repository_url}",
    );
    assert!(
        !repository_url.contains("openai/codex"),
        "codex-cli/package.json `repository.url` must not point at the \
         upstream openai/codex repo. Was: {repository_url}",
    );
}

/// Pin: the npm entrypoint shims route platform packages and reinstall
/// guidance through the fork-owned npm scope. These files are what users hit
/// first after a global npm/bun install, so they must not drift back to an
/// upstream package name during a merge.
#[test]
fn fork_identity_pin_codex_cli_bin_shims() {
    const CODEX_JS: &str = include_str!("../../../codex-cli/bin/codex.js");
    const CODEX_EXEC_JS: &str = include_str!("../../../codex-cli/bin/codex-exec.js");
    let sources = [("codex.js", CODEX_JS), ("codex-exec.js", CODEX_EXEC_JS)];

    for (path, source) in sources {
        for package in [
            "@mmmbuto/codex-vl-linux-x64",
            "@mmmbuto/codex-vl-android-arm64",
            "@mmmbuto/codex-vl-darwin-arm64",
        ] {
            assert!(
                source.contains(package),
                "{path} must route target triples to fork platform package `{package}`",
            );
        }
        assert!(
            source.contains("@mmmbuto/codex-vl@latest"),
            "{path} reinstall/update guidance must use @mmmbuto/codex-vl@latest",
        );
        assert!(
            !source.contains("@mmmbuto/codex-vl@next"),
            "{path} user-facing reinstall/update guidance must not point stable users at @next",
        );
        assert!(
            !source.contains(concat!("@openai", "/codex"))
                && !source.contains(concat!("openai", "/", "codex")),
            "{path} must not route npm users back to upstream Codex packages or repo slugs",
        );
    }
}
