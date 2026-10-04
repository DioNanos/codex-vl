#!/usr/bin/env python3

import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from codex_package import cargo as cargo_module
from codex_package.cargo import build_source_binaries
from codex_package.cargo import source_binaries_for_target
from codex_package.targets import PACKAGE_VARIANTS
from codex_package.targets import TARGET_SPECS


class SourceBinariesForTargetTest(unittest.TestCase):
    def test_macos_package_with_prebuilt_entrypoint_builds_nothing(self) -> None:
        self.assertEqual(
            source_binaries_for_target(
                TARGET_SPECS["aarch64-apple-darwin"],
                PACKAGE_VARIANTS["codex"],
                build_entrypoint=False,
                build_code_mode_host=False,
                build_bwrap=False,
                build_codex_command_runner=False,
                build_codex_windows_sandbox_setup=False,
            ),
            [],
        )

    def test_linux_package_with_prebuilt_entrypoint_and_bwrap_builds_nothing(
        self,
    ) -> None:
        self.assertEqual(
            source_binaries_for_target(
                TARGET_SPECS["x86_64-unknown-linux-musl"],
                PACKAGE_VARIANTS["codex"],
                build_entrypoint=False,
                build_code_mode_host=False,
                build_bwrap=False,
                build_codex_command_runner=False,
                build_codex_windows_sandbox_setup=False,
            ),
            [],
        )

    def test_windows_package_with_prebuilt_entrypoint_and_helpers_builds_nothing(
        self,
    ) -> None:
        self.assertEqual(
            source_binaries_for_target(
                TARGET_SPECS["x86_64-pc-windows-msvc"],
                PACKAGE_VARIANTS["codex"],
                build_entrypoint=False,
                build_code_mode_host=False,
                build_bwrap=False,
                build_codex_command_runner=False,
                build_codex_windows_sandbox_setup=False,
            ),
            [],
        )

    def test_missing_windows_helpers_are_built(self) -> None:
        self.assertEqual(
            source_binaries_for_target(
                TARGET_SPECS["x86_64-pc-windows-msvc"],
                PACKAGE_VARIANTS["codex"],
                build_entrypoint=False,
                build_code_mode_host=False,
                build_bwrap=False,
                build_codex_command_runner=True,
                build_codex_windows_sandbox_setup=True,
            ),
            ["codex-command-runner", "codex-windows-sandbox-setup"],
        )

    def test_missing_code_mode_host_is_built_for_app_server(self) -> None:
        self.assertEqual(
            source_binaries_for_target(
                TARGET_SPECS["aarch64-apple-darwin"],
                PACKAGE_VARIANTS["codex-app-server"],
                build_entrypoint=False,
                build_code_mode_host=True,
                build_bwrap=False,
                build_codex_command_runner=False,
                build_codex_windows_sandbox_setup=False,
            ),
            ["codex-code-mode-host"],
        )

    def test_build_uses_prebuilt_windows_helpers_without_running_cargo(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            entrypoint = touch_file(root / "codex.exe")
            code_mode_host = touch_file(root / "codex-code-mode-host.exe")
            command_runner = touch_file(root / "codex-command-runner.exe")
            sandbox_setup = touch_file(root / "codex-windows-sandbox-setup.exe")

            outputs = build_source_binaries(
                TARGET_SPECS["x86_64-pc-windows-msvc"],
                PACKAGE_VARIANTS["codex"],
                cargo=str(root / "cargo-that-should-not-run"),
                profile="release",
                entrypoint_bin=entrypoint,
                code_mode_host_bin=code_mode_host,
                bwrap_bin=None,
                codex_command_runner_bin=command_runner,
                codex_windows_sandbox_setup_bin=sandbox_setup,
            )

        self.assertEqual(outputs.entrypoint_bin, entrypoint)
        self.assertEqual(outputs.code_mode_host_bin, code_mode_host)
        self.assertEqual(outputs.codex_command_runner_bin, command_runner)
        self.assertEqual(outputs.codex_windows_sandbox_setup_bin, sandbox_setup)


class SourceBuildV8SandboxGuardTest(unittest.TestCase):
    """The build path judges the archive cargo will LINK, on both branches.

    The resolver returns {} on purpose for paired overrides (upstream
    contract) and lets cargo inherit os.environ: judging only its output would
    leave the override unjudged while cargo still links it. These tests pin the
    guard to the build path, not to the resolver.
    """

    def _build(self, root: Path) -> None:
        build_source_binaries(
            TARGET_SPECS["aarch64-apple-darwin"],
            PACKAGE_VARIANTS["codex"],
            cargo=str(root / "cargo"),
            profile="release",
            entrypoint_bin=None,
            code_mode_host_bin=None,
            bwrap_bin=None,
            codex_command_runner_bin=None,
            codex_windows_sandbox_setup_bin=None,
        )

    def test_paired_override_is_judged_before_cargo(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive = touch_file(root / "override-archive.a")
            binding = touch_file(root / "override-binding.rs")
            with (
                mock.patch.dict(os.environ, {
                    "RUSTY_V8_ARCHIVE": str(archive),
                    "RUSTY_V8_SRC_BINDING_PATH": str(binding),
                }),
                mock.patch.object(cargo_module, "assert_sandbox_archive") as guard,
                mock.patch.object(cargo_module.subprocess, "run") as run,
                mock.patch.object(cargo_module, "validate_source_outputs"),
            ):
                self._build(root)
            guard.assert_called_once_with(archive)
            run.assert_called_once()

    def test_unjudgeable_override_stops_the_build_before_cargo(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive = touch_file(root / "override-archive.a")
            binding = touch_file(root / "override-binding.rs")
            with (
                mock.patch.dict(os.environ, {
                    "RUSTY_V8_ARCHIVE": str(archive),
                    "RUSTY_V8_SRC_BINDING_PATH": str(binding),
                }),
                mock.patch.object(
                    cargo_module,
                    "assert_sandbox_archive",
                    side_effect=RuntimeError("cannot judge this archive"),
                ) as guard,
                mock.patch.object(cargo_module.subprocess, "run") as run,
                mock.patch.object(cargo_module, "validate_source_outputs"),
            ):
                with self.assertRaises(RuntimeError):
                    self._build(root)
            guard.assert_called_once_with(archive)
            run.assert_not_called()

    def test_downloaded_archive_is_judged_before_cargo(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive = touch_file(root / "downloaded-archive.a")
            binding = touch_file(root / "downloaded-binding.rs")
            resolved = {
                "RUSTY_V8_ARCHIVE": str(archive),
                "RUSTY_V8_SRC_BINDING_PATH": str(binding),
            }
            with (
                mock.patch.object(
                    cargo_module, "resolve_codex_v8_cargo_env", return_value=resolved
                ),
                mock.patch.object(cargo_module, "assert_sandbox_archive") as guard,
                mock.patch.object(cargo_module.subprocess, "run") as run,
                mock.patch.object(cargo_module, "validate_source_outputs"),
            ):
                self._build(root)
            guard.assert_called_once_with(archive)
            run.assert_called_once()


def touch_file(path: Path) -> Path:
    path.write_text("", encoding="utf-8")
    return path.resolve()


if __name__ == "__main__":
    unittest.main()
