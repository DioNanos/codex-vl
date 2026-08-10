"""The macOS postinstall must pin the same V8 the rest of the build pins.

`postinstall_darwin_build.js` downloads and links its own V8 on the user's
machine, with its own copy of the checksums. Nothing connects that copy to
`third_party/v8/rusty_v8_<version>.sha256`, which is where every other consumer
reads the pin from, so the two can drift silently -- and the way they drift that
matters is one of them ending up on the plain profile, which links and runs with
the sandbox absent and says nothing about it.

There is no semantic check on the user's Mac on purpose: the postinstall would
have to grow a Python dependency to run one. Pinning by checksum is enough there
*provided* the checksum is the same one the release tooling verifies.
"""

from __future__ import annotations

from pathlib import Path
import re
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from codex_package.targets import REPO_ROOT

POSTINSTALL = REPO_ROOT / "codex-cli" / "scripts" / "postinstall_darwin_build.js"
PROFILE = "ptrcomp_sandbox_release"
TARGET = "aarch64-apple-darwin"


def read_postinstall() -> str:
    return POSTINSTALL.read_text(encoding="utf-8")


class DarwinPostinstallPins(unittest.TestCase):
    def test_uses_the_sandbox_profile(self) -> None:
        source = read_postinstall()
        self.assertIn(
            f"librusty_v8_{PROFILE}_${{target}}.a.gz",
            source,
            "the macOS postinstall must fetch the sandbox profile; the plain one "
            "links and runs with the sandbox absent",
        )
        self.assertNotIn("librusty_v8_release_${target}", source)

    def test_checksums_match_the_canonical_manifest(self) -> None:
        source = read_postinstall()

        version_match = re.search(r'const v8Version = "([0-9.]+)"', source)
        self.assertIsNotNone(version_match, "cannot find v8Version")
        version = version_match.group(1)

        block = re.search(r"const v8Checksums = \{(.*?)\};", source, re.S)
        self.assertIsNotNone(block, "cannot find the v8Checksums object")
        digests = re.findall(r'"([0-9a-f]{64})"', block.group(1))
        self.assertEqual(
            len(digests),
            2,
            f"expected an archive and a binding digest, found {len(digests)}",
        )
        archive_digest, binding_digest = digests

        manifest = (
            REPO_ROOT
            / "third_party"
            / "v8"
            / f"rusty_v8_{version.replace('.', '_')}.sha256"
        )
        self.assertTrue(manifest.is_file(), f"{manifest} is missing")
        pinned = {}
        for line in manifest.read_text(encoding="utf-8").splitlines():
            parts = line.split()
            if len(parts) == 2:
                pinned[parts[1]] = parts[0]

        archive_name = f"librusty_v8_{PROFILE}_{TARGET}.a.gz"
        binding_name = f"src_binding_{PROFILE}_{TARGET}.rs"
        self.assertIn(
            archive_name, pinned, f"{archive_name} is not pinned in {manifest}"
        )
        self.assertIn(
            binding_name, pinned, f"{binding_name} is not pinned in {manifest}"
        )
        self.assertEqual(
            archive_digest,
            pinned[archive_name],
            "the macOS postinstall pins a different archive than the release "
            "tooling verifies",
        )
        self.assertEqual(
            binding_digest,
            pinned[binding_name],
            "the macOS postinstall pins a different binding than the release "
            "tooling verifies",
        )


if __name__ == "__main__":
    unittest.main()
