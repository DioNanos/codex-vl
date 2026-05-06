# Codex-VL Upstream Merge Audit - 2026-05-06

## Scope

Audit for the `0.128.4` upstream merge and `next` release preparation.

Required fork surfaces checked:

- `/vivling` command, Vivling state, lifecycle, CRT speech, brain profile routing, and assist/loop brain execution
- `/loop` command, persisted loop jobs, Vivling loop owner routing, and dynamic `manage_loops`
- `/goal` UI, state, protocol events, state accounting, and model tools
- Linux x64 and Termux Android arm64 npm package path
- macOS local source-build npm package and install path

## Upstream Merge Notes

- Upstream `main` was merged into the fork integration branch for `0.128.4`.
- Upstream split app-server protocol `v2.rs` into `protocol/v2/*`; the fork now uses the split files.
- Upstream added bundled `bwrap`; Linux package staging was updated so `codex-linux-x64` carries `codex-resources/bwrap`.
- `rusty-v8-v147.4.0` is an upstream release artifact, but the current merged Rust dependency remains `v8 146.4.0`; no local V8 alignment to 147 is required until the Rust crate dependency moves.
- Termux Android V8 artifacts remain pointed at `rusty-v8-v146.4.0`.

## Preserved Fork Paths

Verified by repository search:

- `codex-rs/tui/src/vivling.rs`
- `codex-rs/tui/src/vivling/`
- `codex-rs/tui/src/vl/`
- `codex-rs/tui/src/app/vl_handler.rs`
- `codex-rs/tui/src/app/background_requests.rs`
- `codex-rs/tui/src/app/loop_controller.rs`
- `codex-rs/tui/src/chatwidget/loop_jobs.rs`
- `codex-rs/state/migrations/0930_vl_thread_loop_jobs.sql`
- `codex-rs/state/migrations/0931_vl_thread_loop_owners.sql`
- `codex-rs/state/migrations/0029_thread_goals.sql`
- `scripts/fetch_rusty_v8_android.py`
- `third_party/v8/android-artifacts.toml`
- `.github/workflows/package-linux-termux.yml`

## Checks Completed

- `python3 -m py_compile codex-cli/scripts/build_npm_package.py scripts/stage_npm_packages.py scripts/fetch_rusty_v8_android.py`
- `python3 .github/scripts/rusty_v8_bazel.py resolved-v8-crate-version`
- `cargo fmt --manifest-path codex-rs/Cargo.toml --all`
- `cargo check --manifest-path codex-rs/Cargo.toml -p codex-core -p codex-tui -p codex-app-server -p codex-tools -p codex-state -p codex-bwrap`
- `cargo test --manifest-path codex-rs/Cargo.toml -p codex-tools dynamic_tools -- --nocapture`
- `cargo test --manifest-path codex-rs/Cargo.toml -p codex-tools goal_tools -- --nocapture`
- `cargo test --manifest-path codex-rs/Cargo.toml -p codex-app-server dynamic_tools -- --nocapture`
- `cargo test --manifest-path codex-rs/Cargo.toml -p codex-state goals -- --nocapture`

## Results

- Runtime compile gate: passed.
- Dynamic `manage_loops`: passed in tool registry and app-server `thread/start` plus `thread/resume`.
- Goal state/accounting: passed.
- Goal tool feature gating: passed after updating the test to explicitly disable the now-default stable `goals` feature for the negative case.
- TUI test harness: blocked before filtered tests run. `codex-tui` runtime compiles, but `lib test` still has upstream split fallout in legacy test-only imports and fixtures.

## Release Gates Still Required

- Optimized Linux x64 build and local npm install/runtime verification: complete.
- Optimized Termux Android arm64 build and package/linkage verification: complete.
- macOS arm64 source-build npm package staging: complete.
- npm publish with `next` for root package and platform tags for native packages: complete.
- Clean merge commit and push only to Forge `origin/develop`: pending at report update time.

## Release Completion Update - 2026-05-06

- Linux x64 static PIE binaries built with maximum release profile and explicit
  `x86_64-linux-musl-gcc` linker.
- Linux npm tarball verified by temporary install and `codex-cli 0.128.4`
  version check.
- Android arm64 binaries built with NDK r27c, API 24 clang, maximum release
  profile, and `rusty-v8-v146.4.0` Android artifacts.
- Android npm tarball verified for AArch64 payload and package metadata.
- Darwin arm64 npm tarball staged as a source-build payload for local Mac
  postinstall/build testing.
- npm registry verified after publish:
  - `next`: `0.128.4`
  - `linux-x64`: `0.128.4-linux-x64`
  - `android-arm64`: `0.128.4-android-arm64`
  - `darwin-arm64`: `0.128.4-darwin-arm64`
  - `latest`: unchanged at `0.128.2`
