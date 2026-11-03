#!/usr/bin/env python3

import gzip
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import check_v8_cve_backport

from codex_package import cargo as cargo_module
from codex_package.cargo import build_source_binaries
from codex_package.cargo import source_binaries_for_target
from codex_package.targets import PACKAGE_VARIANTS
from codex_package.targets import TARGET_SPECS
from codex_package.targets import TargetSpec


class SourceBinariesForTargetTest(unittest.TestCase):
    def test_macos_package_with_prebuilt_entrypoint_builds_nothing(self) -> None:
        self.assertEqual(
            source_binaries_for_target(
                TARGET_SPECS["aarch64-apple-darwin"],
                PACKAGE_VARIANTS["codex"],
                build_entrypoint=False,
                build_code_mode_host=False,
                build_exec_server=False,
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
                build_exec_server=False,
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
                build_exec_server=False,
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
                build_exec_server=False,
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
                build_exec_server=True,
                build_bwrap=False,
                build_codex_command_runner=False,
                build_codex_windows_sandbox_setup=False,
            ),
            ["codex-code-mode-host", "codex-exec-server"],
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
                exec_server_bin=code_mode_host,
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
            exec_server_bin=None,
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
                mock.patch.dict(
                    os.environ,
                    {
                        "RUSTY_V8_ARCHIVE": str(archive),
                        "RUSTY_V8_SRC_BINDING_PATH": str(binding),
                    },
                ),
                mock.patch.object(cargo_module, "assert_sandbox_archive") as guard,
                mock.patch.object(
                    cargo_module, "assert_cve_backport_archive"
                ) as backport_guard,
                mock.patch.object(cargo_module.subprocess, "run") as run,
                mock.patch.object(cargo_module, "validate_source_outputs"),
            ):
                self._build(root)
            guard.assert_called_once_with(archive)
            backport_guard.assert_called_once_with(archive)
            run.assert_called_once()

    def test_unjudgeable_override_stops_the_build_before_cargo(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive = touch_file(root / "override-archive.a")
            binding = touch_file(root / "override-binding.rs")
            with (
                mock.patch.dict(
                    os.environ,
                    {
                        "RUSTY_V8_ARCHIVE": str(archive),
                        "RUSTY_V8_SRC_BINDING_PATH": str(binding),
                    },
                ),
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
                mock.patch.object(
                    cargo_module, "assert_cve_backport_archive"
                ) as backport_guard,
                mock.patch.object(cargo_module.subprocess, "run") as run,
                mock.patch.object(cargo_module, "validate_source_outputs"),
            ):
                self._build(root)
            guard.assert_called_once_with(archive)
            backport_guard.assert_called_once_with(archive)
            run.assert_called_once()


class SourceBuildV8BackportGateTest(unittest.TestCase):
    """The same boundary also judges the CVE backport marker, behaviorally.

    The sandbox verdict is mocked to its ok outcome so only the backport
    gate's decision is under test. The backport gate itself runs for real --
    `assert_cve_backport_archive` shells out to scripts/check_v8_cve_backport.py
    on an archive that exists on disk -- and cargo is a fake executable that
    records being invoked: reaching it means every guard passed, not reaching
    it means the build stopped before cargo. The archives are small gzip
    stand-ins shaped like a staged rusty_v8 pair member, so the suite needs
    no network, no toolchain, and no release artifact.
    """

    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.cargo = self.root / "fake-cargo"
        self.log = self.root / "cargo-invocations.log"
        self.cargo.write_text(
            '#!/bin/sh\nprintf \'%s\\n\' "$*" >> "$FAKE_CARGO_LOG"\n',
            encoding="utf-8",
        )
        self.cargo.chmod(0o755)

    def _archive(self, name: str, *, with_marker: bool) -> Path:
        # Same shape the marker-scrubbing counterproof used: the marker line
        # is replaced byte-for-byte, so size and layout cannot tell the two
        # apart -- only the scanner's verdict can.
        body = b"!<arch>\n" + b"placeholder object payload\n" * 64
        marker = check_v8_cve_backport.MARKER
        trailer = marker if with_marker else b"X" * len(marker)
        archive = self.root / name
        archive.write_bytes(gzip.compress(body + b"\n" + trailer + b"\n"))
        return archive

    def _build(
        self, spec: TargetSpec = TARGET_SPECS["x86_64-unknown-linux-musl"]
    ) -> None:
        build_source_binaries(
            spec,
            PACKAGE_VARIANTS["codex"],
            cargo=str(self.cargo),
            profile="release",
            entrypoint_bin=None,
            code_mode_host_bin=None,
            exec_server_bin=None,
            bwrap_bin=None,
            codex_command_runner_bin=None,
            codex_windows_sandbox_setup_bin=None,
        )

    def _cargo_invocations(self) -> int:
        if not self.log.exists():
            return 0
        return len(self.log.read_text(encoding="utf-8").splitlines())

    def test_valid_archive_with_marker_builds(self) -> None:
        archive = self._archive("linked-archive.a.gz", with_marker=True)
        resolved = {
            "RUSTY_V8_ARCHIVE": str(archive),
            "RUSTY_V8_SRC_BINDING_PATH": str(self.root / "binding.rs"),
        }
        with (
            mock.patch.object(
                cargo_module, "resolve_codex_v8_cargo_env", return_value=resolved
            ),
            mock.patch.object(cargo_module, "assert_sandbox_archive") as sandbox,
            mock.patch.object(cargo_module, "validate_source_outputs"),
            mock.patch.dict(os.environ, {"FAKE_CARGO_LOG": str(self.log)}),
        ):
            self._build()
        sandbox.assert_called_once_with(archive)
        self.assertEqual(self._cargo_invocations(), 1)

    def test_marker_scrubbed_archive_stops_before_cargo(self) -> None:
        archive = self._archive("linked-archive.a.gz", with_marker=False)
        resolved = {
            "RUSTY_V8_ARCHIVE": str(archive),
            "RUSTY_V8_SRC_BINDING_PATH": str(self.root / "binding.rs"),
        }
        with (
            mock.patch.object(
                cargo_module, "resolve_codex_v8_cargo_env", return_value=resolved
            ),
            mock.patch.object(cargo_module, "assert_sandbox_archive") as sandbox,
            mock.patch.object(cargo_module, "validate_source_outputs"),
            mock.patch.dict(os.environ, {"FAKE_CARGO_LOG": str(self.log)}),
        ):
            with self.assertRaisesRegex(RuntimeError, "marker absent"):
                self._build()
        sandbox.assert_called_once_with(archive)
        self.assertEqual(self._cargo_invocations(), 0)

    def test_truncated_archive_stops_before_cargo(self) -> None:
        archive = self._archive("linked-archive.a.gz", with_marker=True)
        blob = archive.read_bytes()
        archive.write_bytes(blob[: len(blob) // 2])
        resolved = {
            "RUSTY_V8_ARCHIVE": str(archive),
            "RUSTY_V8_SRC_BINDING_PATH": str(self.root / "binding.rs"),
        }
        with (
            mock.patch.object(
                cargo_module, "resolve_codex_v8_cargo_env", return_value=resolved
            ),
            mock.patch.object(cargo_module, "assert_sandbox_archive"),
            mock.patch.object(cargo_module, "validate_source_outputs"),
            mock.patch.dict(os.environ, {"FAKE_CARGO_LOG": str(self.log)}),
        ):
            with self.assertRaisesRegex(RuntimeError, "cannot decompress"):
                self._build()
        self.assertEqual(self._cargo_invocations(), 0)

    def test_windows_target_never_runs_the_backport_gate(self) -> None:
        # Marker-less on purpose: the Windows archive is upstream and carries
        # no marker by construction, so the build may only proceed because
        # the backport gate is skipped there, not despite it.
        archive = self._archive("windows-archive.lib.gz", with_marker=False)
        resolved = {
            "RUSTY_V8_ARCHIVE": str(archive),
            "RUSTY_V8_SRC_BINDING_PATH": str(self.root / "binding.rs"),
        }
        with (
            mock.patch.object(
                cargo_module, "resolve_codex_v8_cargo_env", return_value=resolved
            ),
            mock.patch.object(cargo_module, "assert_sandbox_archive") as sandbox,
            mock.patch.object(
                cargo_module, "assert_cve_backport_archive"
            ) as backport_guard,
            mock.patch.object(cargo_module, "validate_source_outputs"),
            mock.patch.dict(os.environ, {"FAKE_CARGO_LOG": str(self.log)}),
        ):
            self._build(TARGET_SPECS["x86_64-pc-windows-msvc"])
        sandbox.assert_called_once_with(archive)
        backport_guard.assert_not_called()
        self.assertEqual(self._cargo_invocations(), 1)

    def test_paired_override_is_gated_on_the_override_archive(self) -> None:
        # The override path from the audit: the resolver returns {} on
        # purpose for paired overrides, cargo inherits os.environ, and the
        # marker-scrubbed override must stop the build before cargo runs.
        archive = self._archive("override-archive.a.gz", with_marker=False)
        with (
            mock.patch.dict(
                os.environ,
                {
                    "RUSTY_V8_ARCHIVE": str(archive),
                    "RUSTY_V8_SRC_BINDING_PATH": str(
                        self.root / "override-binding.rs"
                    ),
                    "FAKE_CARGO_LOG": str(self.log),
                },
            ),
            mock.patch.object(cargo_module, "assert_sandbox_archive") as sandbox,
            mock.patch.object(cargo_module, "validate_source_outputs"),
        ):
            with self.assertRaisesRegex(RuntimeError, "marker absent"):
                self._build()
        sandbox.assert_called_once_with(archive)
        self.assertEqual(self._cargo_invocations(), 0)

    def test_paired_override_with_marker_builds(self) -> None:
        archive = self._archive("override-archive.a.gz", with_marker=True)
        with (
            mock.patch.dict(
                os.environ,
                {
                    "RUSTY_V8_ARCHIVE": str(archive),
                    "RUSTY_V8_SRC_BINDING_PATH": str(
                        self.root / "override-binding.rs"
                    ),
                    "FAKE_CARGO_LOG": str(self.log),
                },
            ),
            mock.patch.object(cargo_module, "assert_sandbox_archive") as sandbox,
            mock.patch.object(cargo_module, "validate_source_outputs"),
        ):
            self._build()
        sandbox.assert_called_once_with(archive)
        self.assertEqual(self._cargo_invocations(), 1)


def touch_file(path: Path) -> Path:
    path.write_text("", encoding="utf-8")
    return path.resolve()


if __name__ == "__main__":
    unittest.main()
