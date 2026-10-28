#!/bin/bash
set -euo pipefail

# codex-vl fork patch verification.
#
# One guard per fork feature group (A1..A12) and one per feature-register
# area (B1..B16). The guards are static anchors: stable grep patterns over
# files the fork owns or shapes. They are the pre-merge contract — run this
# on the develop tip BEFORE opening an upstream merge (expected: 0 missing)
# and on the merge result afterwards. A guard going missing on the merge
# means either an anchor moved (re-pin it) or a fork patch was silently
# dropped (restore it); both must be explained, never hidden.
#
# Set VERIFY_PATCHES_SKIP_CARGO=1 to skip the cargo-backed guards. The
# coordination host must never run cargo; CI leaves this switch unset and
# the skip is refused there. All static guards still run and the final
# summary determines the exit status.

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT_DIR"

PRESENT=0
MISSING=0
SKIPPED=0

pass() {
  PRESENT=$((PRESENT + 1))
  echo "✅ PRESENT"
}

fail() {
  MISSING=$((MISSING + 1))
  echo "❌ MISSING!"
}

skip() {
  SKIPPED=$((SKIPPED + 1))
  echo "⏭ SKIPPED ($1)"
}

count_in() {
  # grep -c returns exit 1 on zero matches; normalize for arithmetic use.
  grep -c "$1" "$2" 2>/dev/null || true
}

printf "Group A1 (fork identity crate + sanitization gate): "
if [ -f codex-rs/fork-gate/Cargo.toml ] \
  && grep -q '^name = "fork-gate"' codex-rs/fork-gate/Cargo.toml \
  && [ -f codex-rs/fork-gate/src/lib.rs ] \
  && [ -f codex-rs/fork-gate/tests/sanitization_gate.rs ] \
  && grep -q '"fork-gate"' codex-rs/Cargo.toml; then
  pass
else
  fail
fi

printf "Group A2 (Vivling runtime crates and TUI modules): "
if [ -f codex-rs/vivling-core/Cargo.toml ] \
  && grep -q '^name = "codex-vivling-core"' codex-rs/vivling-core/Cargo.toml \
  && [ -d codex-rs/tui/src/vivling ] \
  && grep -q 'mod vivling;' codex-rs/tui/src/lib.rs \
  && grep -q 'mod vl;' codex-rs/tui/src/lib.rs; then
  pass
else
  fail
fi

printf "Group A3 (Vivling delegation injects the live session model): "
# The three LLM spawn sites (assist / loop tick / expression) must build the
# dispatched config through config_with_session_model, never a static
# app.config.clone(): on fresh installs config.model is None and every
# Vivling channel would fail with "session has no default model configured".
delegation_count="$(count_in 'config_with_session_model' \
  codex-rs/tui/src/app/loop_controller/vivling_delegation.rs)"
static_clone_count="$(count_in 'app\.config\.clone()' \
  codex-rs/tui/src/app/loop_controller/vivling_delegation.rs)"
if [ "${delegation_count:-0}" -ge 3 ] && [ "${static_clone_count:-0}" -eq 0 ]; then
  pass
else
  fail
fi

printf "Group A4 (goal lifecycle lives in ext/goal and its consumers): "
# The goal API on this line is set_thread_goal / clear_thread_goal (the
# older create_thread_goal name does not exist since the goal logic moved
# into ext/goal). Consumers in the app-server processor and the TUI actions
# must keep calling through that boundary.
if [ -f codex-rs/ext/goal/src/api.rs ] \
  && grep -q 'pub async fn set_thread_goal' codex-rs/ext/goal/src/api.rs \
  && grep -q 'pub async fn clear_thread_goal' codex-rs/ext/goal/src/api.rs \
  && grep -q 'clear_thread_goal' \
    codex-rs/app-server/src/request_processors/thread_goal_processor.rs \
  && grep -q 'clear_thread_goal' codex-rs/tui/src/app/thread_goal_actions.rs; then
  pass
else
  fail
fi

printf "Group A5 (loop jobs and manage_loops routing): "
# manage_loops must stay routed from the app-server dynamic-tool path back
# to the TUI loop controller, goal_text must parse, and the resume path must
# await the (async upstream) thread-event-state reset.
if [ "$(count_in 'manage_loops' \
    codex-rs/app-server/src/request_processors/thread_processor.rs)" -ge 5 ] \
  && grep -q 'goal_text' codex-rs/tui/src/app/loop_controller/parsing.rs \
  && grep -q 'loop_state_runtime' codex-rs/tui/src/app/loop_controller/mod.rs \
  && grep -q 'app\.reset_thread_event_state()\.await;' \
    codex-rs/tui/src/app/loop_controller/manage_tool.rs; then
  pass
else
  fail
fi

printf "Group A6 (GoalStore boundary and ThreadLoopJob exports): "
if grep -q 'pub use model::ThreadLoopJob' codex-rs/state/src/lib.rs \
  && grep -q 'pub use model::THREAD_LOOP_OWNER_KIND_VIVLING' codex-rs/state/src/lib.rs \
  && grep -q 'fn thread_goals' codex-rs/state/src/runtime.rs \
  && grep -q 'GoalStore' codex-rs/state/src/runtime.rs \
  && [ -f codex-rs/state/src/runtime/goals.rs ]; then
  pass
else
  fail
fi

printf "Group A7 (app-server connection identity): "
if grep -q 'identity_required' codex-rs/app-server/src/lib.rs \
  && grep -q 'identity_required' codex-rs/app-server/src/in_process.rs \
  && grep -q 'identity_required' \
    codex-rs/app-server/src/message_processor_tracing_tests.rs \
  && grep -q 'identity_required' \
    codex-rs/app-server/src/message_processor_workspace_routing_tests.rs \
  && grep -q 'mod identity_handshake;' codex-rs/app-server-client/src/lib.rs \
  && [ -f codex-rs/app-server/tests/terminal_identity.rs ] \
  && [ -f codex-rs/app-server/src/bin/identity_test_daemon_shim.rs ] \
  && grep -q 'nexuscrew/identity/bind' \
    codex-rs/app-server/tests/common/identity_fixture.rs; then
  pass
else
  fail
fi

printf "Group A8 (VL UI surfaces: events, slash modules, footer lane, hints): "
if grep -q 'pub(crate) enum VlEvent' codex-rs/tui/src/vl/events.rs \
  && [ -f codex-rs/tui/src/chatwidget/vl_ext.rs ] \
  && [ -f codex-rs/tui/src/chatwidget/vl_slash.rs ] \
  && [ -f codex-rs/tui/src/bottom_pane/chat_composer/vl_footer.rs ] \
  && grep -q 'loop_context_renders_below_status_line_without_replacing_it' \
    codex-rs/tui/src/bottom_pane/chat_composer/vl_footer.rs \
  && grep -q 'crate::chat_hint::hint_for' codex-rs/tui/src/chatwidget/constructor.rs \
  && grep -q 'crate::chat_hint::hint_for' \
    codex-rs/tui/src/bottom_pane/async_questions/render.rs; then
  pass
else
  fail
fi

printf "Group A9 (daemon pairing guard): "
# Pairing attaches to a running daemon and must refuse through
# ensure_pairing_daemon_socket, naming the missing socket and the
# `remote-control start` command. Upstream does not carry this guard, so an
# upstream rewrite of that function drops it silently.
if [ "$(count_in 'ensure_pairing_daemon_socket' \
    codex-rs/app-server-daemon/src/lib.rs)" -ge 2 ] \
  && grep -q 'pairing_without_a_running_daemon_says_how_to_start_one' \
    codex-rs/app-server-daemon/src/lib.rs \
  && grep -q 'run `remote-control start` first' \
    codex-rs/app-server-daemon/src/lib.rs; then
  pass
else
  fail
fi

printf "Group A10 (npm packaging identity and musl ripgrep): "
if grep -q '"name": "@mmmbuto/codex-vl"' codex-cli/package.json \
  && grep -q 'DioNanos/codex-vl' codex-cli/package.json \
  && [ -f codex-cli/scripts/build_npm_package.py ] \
  && [ -f scripts/stage_npm_packages.py ] \
  && grep -q 'mmmbuto%2fcodex-vl' codex-rs/tui/src/npm_registry.rs \
  && ! grep -q 'aarch64-unknown-linux-gnu' scripts/codex_package/rg \
  && [ "$(count_in 'aarch64-unknown-linux-musl' \
      scripts/codex_package/rg)" -ge 2 ]; then
  pass
else
  fail
fi

# The ripgrep anchor is version-agnostic on purpose: the musl aarch64
# package must never ship the gnu artifact. A merge that takes the upstream
# manifest restores it with no compilation signal, and rg then fails at
# runtime on the device.

printf "Group A11 (fork CI overlay): "
# The overlay patch must be regenerated after every merge that touches the
# upstream workflows; these anchors only pin that the overlay mechanism
# itself (guard workflow + patch + checker) is intact.
if [ -f .github/workflows/fork-overlay-guard.yml ] \
  && grep -q 'check_fork_ci_overlay.py' .github/workflows/fork-overlay-guard.yml \
  && [ -f scripts/fork-overlay/check_fork_ci_overlay.py ] \
  && [ -f scripts/fork-overlay/fork-ci-overlay.patch ]; then
  pass
else
  fail
fi

printf "Group A12 (fork crates stay linked into the workspace): "
# Static stand-in for `cargo shear`: a merge can drop a member or a mod link
# while the files stay on disk, and then the code is invisible to the
# compiler with every check green. The full detector runs in CI.
if grep -q '"vivling-core",' codex-rs/Cargo.toml \
  && grep -q '"fork-gate",' codex-rs/Cargo.toml \
  && grep -q 'codex-vivling-core' codex-rs/Cargo.toml \
  && grep -q 'mod vivling;' codex-rs/tui/src/lib.rs \
  && grep -q 'mod vl;' codex-rs/tui/src/lib.rs; then
  pass
else
  fail
fi

printf "Area B1 (core goal lifecycle): "
if grep -q 'CreateGoalRequest' codex-rs/ext/goal/src/tool.rs \
  && grep -q 'set_thread_goal' codex-rs/ext/goal/src/api.rs \
  && grep -q 'clear_thread_goal' codex-rs/ext/goal/src/api.rs \
  && [ -f codex-rs/tui/src/app/thread_goal_actions.rs ] \
  && [ -f codex-rs/tui/src/chatwidget/goal_menu.rs ]; then
  pass
else
  fail
fi

printf "Area B2 (GoalStore and state boundary): "
if grep -q 'GoalStore' codex-rs/state/src/runtime.rs \
  && grep -q 'pub use model::ThreadLoopJob' codex-rs/state/src/lib.rs \
  && grep -q 'pub use model::ThreadLoopJobCreateParams' codex-rs/state/src/lib.rs \
  && [ -f codex-rs/state/src/runtime/goals.rs ]; then
  pass
else
  fail
fi

printf "Area B3 (manage_loops flat + namespaced, routing regression pins): "
if grep -q 'codex_app::manage_loops' codex-rs/core/src/tools/spec_plan_tests.rs \
  && grep -q 'manage_loops' codex-rs/core/src/tools/spec_plan_tests.rs \
  && grep -q 'gate-fix-probe' codex-rs/tui/src/app/tests/loop_tool_routing.rs \
  && grep -q 'goal_text' codex-rs/tui/src/app/loop_controller/parsing.rs; then
  pass
else
  fail
fi

printf "Area B4 (Vivling runtime symbols and strict brain target config): "
if grep -q 'VivlingLoopTickRequest' \
    codex-rs/tui/src/app/loop_controller/vivling_delegation.rs \
  && grep -q 'VivlingLoopTickRequest' codex-rs/tui/src/app/vivling_background.rs \
  && grep -q 'record_vivling_loop_runtime' \
    codex-rs/tui/src/app/loop_controller/vivling_delegation.rs \
  && [ "$(count_in 'resolve_vivling_brain_target_config' \
      codex-rs/tui/src/app/vivling_background.rs)" -ge 3 ]; then
  pass
else
  fail
fi

printf "Area B5 (VL UI boundaries and the background client seam): "
# The TUI must not construct AuthManager/ThreadManager itself: background
# Vivling clients go through the core-owned build_background_model_client
# boundary. manager_dependency_regression pins this in CI.
if [ -f codex-rs/tui/tests/manager_dependency_regression.rs ] \
  && grep -Eq 'AuthManager|build_background_model_client' \
    codex-rs/tui/tests/manager_dependency_regression.rs \
  && grep -q 'fn build_background_model_client' codex-rs/core/src/thread_manager.rs \
  && [ "$(count_in 'build_background_model_client' \
      codex-rs/tui/src/app/vivling_background.rs)" -ge 3 ] \
  && ! grep -rq 'AuthManager' codex-rs/tui/src \
  && [ -f codex-rs/tui/tests/fork_identity_pins.rs ]; then
  pass
else
  fail
fi

printf "Area B6 (remote-control surfaces in the app-server): "
if [ -f codex-rs/app-server/src/request_processors/thread_goal_processor.rs ] \
  && grep -rq 'thread_goals()\|GoalStore' codex-rs/app-server/src \
  && [ "$(count_in 'manage_loops' \
      codex-rs/app-server/src/request_processors/thread_processor.rs)" -ge 5 ]; then
  pass
else
  fail
fi

printf "Area B7 (fork identity on public surfaces): "
if grep -q '"name": "@mmmbuto/codex-vl"' codex-cli/package.json \
  && grep -q '@mmmbuto/codex-vl@' README.md \
  && grep -q 'DioNanos/codex-vl' scripts/install/install.sh \
  && ! grep -q '@openai/codex' README.md \
  && ! grep -q '@openai/codex' codex-cli/package.json \
  && ! grep -q '@openai/codex' package.json; then
  pass
else
  fail
fi

printf "Area B8 (model-message caps, 8 KiB fail-closed): "
if grep -q 'pub const MODEL_MESSAGE_TEXT_MAX_BYTES' \
    codex-rs/protocol/src/openai_models.rs \
  && grep -q 'fn validate_model_message_text' codex-rs/protocol/src/openai_models.rs \
  && grep -q 'validate_model_message_text' \
    codex-rs/core/src/context/guardian_node_repl_policy.rs \
  && grep -q 'validate_model_message_text' \
    codex-rs/core/src/context/world_state/persistent_mode.rs \
  && grep -q 'partition_model_infos' codex-rs/codex-api/src/endpoint/models.rs \
  && grep -q 'partition_model_infos' codex-rs/models-manager/src/cache.rs; then
  pass
else
  fail
fi

printf "Area B9 (catalog instructions: legacy promotion, fail-closed load): "
# ensure_catalog_instructions is deliberately ABSENT: the obsolete embedded
# fallback was removed and the guarantee lives in the deserializer's fatal
# branch. Its return would mask broken catalogs again.
if [ "$(count_in 'deserialize_model_infos_with_legacy_base' \
    codex-rs/protocol/src/openai_models.rs)" -ge 1 ] \
  && grep -q 'missing both' codex-rs/protocol/src/openai_models.rs \
  && ! grep -rq 'ensure_catalog_instructions' codex-rs \
  && [ "$(count_in 'models_response' \
      codex-rs/protocol/src/openai_models.rs)" -ge 2 ]; then
  pass
else
  fail
fi

printf "Area B10 (windows-sandbox ACL fix): "
if grep -q 'fn grant_read_execute_aces' codex-rs/windows-sandbox-rs/src/acl.rs \
  && grep -q 'fn ensure_runtime_tree_readable' \
    codex-rs/windows-sandbox-rs/src/setup_provisioning/setup_runtime_bin.rs \
  && grep -q 'mod setup_runtime_bin_tests' \
    codex-rs/windows-sandbox-rs/src/setup_provisioning/setup_runtime_bin.rs \
  && [ -f codex-rs/windows-sandbox-rs/src/setup_provisioning/setup_runtime_bin/setup_runtime_bin_tests.rs ]; then
  pass
else
  fail
fi

printf "Area B11 (namespace_tools provider gate, attested counts): "
# Upstream removed this gate once without producing any conflict: the field
# stayed in config while the consumers vanished. The exact counts are the
# contract — spec_plan 8, client 1, world_state 1, capabilities >= 2. A
# count that moves in EITHER direction must be looked at, not auto-accepted.
if [ "$(count_in 'namespace_tools' \
    codex-rs/core/src/tools/spec_plan.rs)" -eq 8 ] \
  && [ "$(count_in 'namespace_tools' codex-rs/core/src/client.rs)" -eq 1 ] \
  && [ "$(count_in 'namespace_tools' \
      codex-rs/core/src/session/world_state.rs)" -eq 1 ] \
  && [ "$(count_in 'namespace_tools' \
      codex-rs/model-provider/src/capabilities.rs)" -ge 2 ]; then
  pass
else
  fail
fi

printf "Area B12 (/remote-control pair and /mcp reload): "
# The fork wires, upstream executes: if McpServerRefresh or the pairing
# request disappear upstream, the wiring compiles but does nothing useful
# and the feature must be redesigned, not stitched back.
if grep -q 'McpServerRefresh' codex-rs/app-server-protocol/src/protocol/common.rs \
  && grep -q 'fn start_pairing' \
    codex-rs/app-server-daemon/src/remote_control_client.rs \
  && grep -q 'RemoteControlPairingStartParams' \
    codex-rs/app-server-daemon/src/remote_control_client.rs \
  && grep -q 'manualPairingCode' codex-rs/tui/src/vl/remote_control.rs \
  && grep -q 'pair_output_shows_the_code_the_app_asks_for' \
    codex-rs/tui/src/vl/remote_control.rs \
  && grep -q 'pair_output_never_offers_the_transport_code_as_a_fallback' \
    codex-rs/tui/src/vl/remote_control.rs \
  && grep -rq 'slash_mcp_reload_emits_global_reload_vl_event' codex-rs/tui/src; then
  pass
else
  fail
fi

printf "Area B13 (advisory locks degrade where the filesystem lacks them): "
# ENOTSUP inventory: eight guarded files. unix_socket and writer_lock carry
# multiple lock sites each, hence the higher floors. An unguarded lock here
# does not fail any test: the CLI just stops starting on the device.
enotsup_ok=1
for f in \
  codex-rs/app-server-transport/src/transport/unix_socket.rs \
  codex-rs/rollout/src/writer_lock.rs \
  codex-rs/message-history/src/lib.rs \
  codex-rs/message-history/src/batch.rs \
  codex-rs/arg0/src/lib.rs \
  codex-rs/execpolicy/src/amend.rs \
  codex-rs/core/src/installation_id.rs \
  codex-rs/app-server-daemon/src/lib.rs; do
  [ "$(count_in 'is_unsupported_file_lock_error' "$f")" -ge 1 ] || enotsup_ok=0
done
if [ "$enotsup_ok" -eq 1 ] \
  && [ "$(count_in 'is_unsupported_file_lock_error' \
      codex-rs/app-server-transport/src/transport/unix_socket.rs)" -ge 2 ] \
  && [ "$(count_in 'is_unsupported_file_lock_error' \
      codex-rs/rollout/src/writer_lock.rs)" -ge 2 ] \
  && grep -q 'ErrorKind::Unsupported' codex-rs/rollout/src/writer_lock.rs; then
  pass
else
  fail
fi

printf "Area B14 (code-mode host travels in the package): "
# Upstream runs code mode out-of-process and fails closed when the host is
# absent. The crate compiles and every test passes with the host missing
# from the payload: only the packaging lists know.
if grep -q -- '-p codex-code-mode-host' .github/workflows/package-linux-termux.yml \
  && grep -q -- '-p codex-code-mode-host' .github/workflows/package-linux-arm64.yml \
  && grep -q 'codex-code-mode-host' codex-cli/scripts/build_npm_package.py; then
  pass
else
  fail
fi

printf "Area B15 (host-cell live seams, exactly one bespoke pass): "
# The per-recipient delivery overlay requires a single
# apply_bespoke_event_handling pass; a merge that unifies the fanout senders
# or adds a second pass makes the partition unrealizable.
if [ "$(count_in 'apply_bespoke_event_handling' \
    codex-rs/app-server/src/request_processors/thread_lifecycle.rs)" -eq 1 ] \
  && grep -q 'fn submit_with_id' codex-rs/core/src/session/mod.rs \
  && grep -q 'fn invalid_request' codex-rs/app-server/src/error_code.rs \
  && grep -rq 'ConnectionOrigin' codex-rs/app-server-transport/src/transport/; then
  pass
else
  fail
fi

printf "Area B16 (launcher env plumbing and identity fds): "
if [ -f codex-cli/bin/identity_fds.js ] \
  && grep -q 'identity_fds.js' codex-cli/bin/codex.js \
  && grep -q 'identity_fds.js' codex-cli/package.json \
  && [ -f codex-rs/utils/process/src/launcher_env.rs ] \
  && grep -q 'NEXUSCREW_IDENTITY_FD' codex-rs/utils/process/src/launcher_env.rs \
  && grep -q 'CODEX_SELF_EXE' codex-cli/bin/codex.js \
  && grep -q 'CODEX_SELF_EXE' codex-cli/bin/codex-exec.js; then
  pass
else
  fail
fi

printf "Area B1 cargo (goal extension suite): "
if [ "${VERIFY_PATCHES_SKIP_CARGO:-0}" = "1" ]; then
  if [ -n "${GITHUB_ACTIONS:-}" ] || [ -n "${CI:-}" ]; then
    echo "skip not allowed in CI"
    fail
  else
    skip "cargo disabled by VERIFY_PATCHES_SKIP_CARGO"
  fi
elif cargo test --manifest-path codex-rs/Cargo.toml -p codex-goal-extension; then
  pass
else
  fail
fi

printf "Area B11 cargo (provider can disable namespace_tools): "
if [ "${VERIFY_PATCHES_SKIP_CARGO:-0}" = "1" ]; then
  if [ -n "${GITHUB_ACTIONS:-}" ] || [ -n "${CI:-}" ]; then
    echo "skip not allowed in CI"
    fail
  else
    skip "cargo disabled by VERIFY_PATCHES_SKIP_CARGO"
  fi
elif cargo test --manifest-path codex-rs/Cargo.toml -p codex-model-provider-info \
    --lib configured_provider_can_disable_namespace_tools; then
  pass
else
  fail
fi

echo "verify-patches: ${PRESENT} present, ${MISSING} missing, ${SKIPPED} skipped"
if [ "$MISSING" -gt 0 ]; then
  exit 1
fi
exit 0
