"""Every packaging consumer of V8 must run the backport marker gate.

`scripts/check_v8_cve_backport.py` exists so that a rusty_v8 archive built
without the Array.sort backport (CVE-2026-85046) can never reach a package:
the producer workflows run it on every artifact they stage. This test keeps
the consumer side wired -- every packaging job that downloads a V8 archive
from the fork's -vl1 tag must also run the gate, fail-closed, and Windows
must never run it, because its archive is the upstream one and is without
the marker by construction.

The assertions are deliberately textual (same approach as test_v8_pins.py):
the wiring lives in shell steps, and what must not regress is the exact
invocation being present in the right legs.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from codex_package.targets import REPO_ROOT

GATE = "check_v8_cve_backport.py"
TERMUX = REPO_ROOT / ".github" / "workflows" / "package-linux-termux.yml"
ARM64 = REPO_ROOT / ".github" / "workflows" / "package-linux-arm64.yml"
SETUP_ACTION = REPO_ROOT / ".github" / "actions" / "setup-rusty-v8" / "action.yml"
WINDOWS_RELEASE = REPO_ROOT / ".github" / "workflows" / "rust-release-windows.yml"
NON_WINDOWS_GUARD = 'if [[ "$TARGET" != *-pc-windows-msvc ]]; then'


class V8BackportGateWiring(unittest.TestCase):
    def test_termux_runs_the_gate_on_both_legs(self) -> None:
        source = TERMUX.read_text(encoding="utf-8")
        self.assertEqual(
            source.count(GATE),
            2,
            "package-linux-termux.yml must run the backport marker gate once in "
            "the x86_64 musl leg and once in the Android leg",
        )

    def test_arm64_runs_the_gate_on_the_musl_leg(self) -> None:
        source = ARM64.read_text(encoding="utf-8")
        self.assertEqual(
            source.count(GATE),
            1,
            "package-linux-arm64.yml must run the backport marker gate in the "
            "aarch64 musl leg",
        )

    def test_setup_action_gates_only_outside_windows(self) -> None:
        source = SETUP_ACTION.read_text(encoding="utf-8")
        self.assertEqual(
            source.count(GATE),
            1,
            "setup-rusty-v8 must run the backport marker gate once, so every "
            "consumer of the shared action (macOS packaging included) is covered "
            "without duplicating the check per workflow",
        )
        guard_at = source.find(NON_WINDOWS_GUARD)
        gate_at = source.find(GATE)
        self.assertGreater(
            guard_at,
            -1,
            "setup-rusty-v8 lost the non-Windows guard around the backport gate",
        )
        self.assertLess(
            guard_at,
            gate_at,
            "the backport gate in setup-rusty-v8 must sit behind the "
            "non-Windows guard: the Windows archive is upstream and is without "
            "the marker by construction",
        )

    def test_windows_release_never_runs_the_gate(self) -> None:
        source = WINDOWS_RELEASE.read_text(encoding="utf-8")
        self.assertNotIn(
            GATE,
            source,
            "rust-release-windows.yml must not run the backport marker gate: "
            "Windows links the upstream archive, which never carries the marker",
        )


if __name__ == "__main__":
    unittest.main()
