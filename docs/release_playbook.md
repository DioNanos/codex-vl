# Release Playbook

This fork keeps release preparation and release execution separate:

- `develop` on Forge carries the full internal work history and release prep
- public release snapshotting happens later, only when the base is stable

For the current stabilization phase, work from `develop` only. Do not treat this
document as a signal to publish or cut release snapshots yet.

## Current target set

The intended first release lane for Codex VL remains intentionally small and
verifiable:

- `macOS arm64` built locally on the Mac Air
- `Linux x64` prepared for the Forge/VPS1 lane
- `Android arm64` prepared for the Forge/VPS1 lane

This matches the fork's npm platform packaging focus: one thin root package plus
three platform payload packages.

## macOS arm64 local package

From the repo root:

```bash
scripts/release/build_macos_local.sh
```

What it does:

- builds `codex` for `aarch64-apple-darwin`
- stages a self-contained local npm tarball with embedded vendor payload
- leaves the installed CLI untouched
- keeps the shared upstream runtime state in `~/.codex`

If you explicitly need a side-by-side local install for manual testing, use the
optional `--install` path separately from release preparation.

## Linux x64 and Android arm64 base

The Linux and Android work should stay release-ready in source now, even before
those lanes are executed. The packaging contract is:

- root package: `@mmmbuto/codex-vl`
- macOS payload: `@mmmbuto/codex-vl-darwin-arm64`
- Linux payload: `@mmmbuto/codex-vl-linux-x64`
- Android payload: `@mmmbuto/codex-vl-android-arm64`

Keep Android/Termux first-class rather than experimental. Preserve the existing
Android baseline in source:

- `termux-open-url`
- Android no-voice behavior
- `$ORIGIN` runtime path
- `CODEX_SELF_EXE`
- sanitized `LD_LIBRARY_PATH`
- Android V8 artifact metadata and fetch tooling

## Runtime validation

After install smoke on each target machine, run:

```bash
qa/runtime-suite/run_cli_runtime_suite.sh
```

For live checks:

```bash
qa/runtime-suite/run_cli_runtime_suite.sh --live
```

Keep the generated report with the release candidate evidence.
