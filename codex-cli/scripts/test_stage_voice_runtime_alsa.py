"""alsa is a Linux-only packaging step: the gnu targets compile it into the
SDK, darwin never touches it. Locks the cmd_stage gate in place."""

import argparse
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import stage_voice_runtime as stage


def _stage_calls(voice_target: str) -> list:
    asset = {"asset": "fixture.tgz", "sha256": "0" * 64, "voiceTarget": voice_target}
    calls = []
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        arguments = argparse.Namespace(
            app_target="fixture-target",
            work=root / "work",
            cache=root / "cache",
        )
        patches = (
            patch.object(stage, "pinned_asset", return_value=asset),
            patch.object(stage, "download_verified", return_value=root / "fixture.tgz"),
            patch.object(stage, "extract_runtime", return_value=root / "runtime"),
            patch.object(stage, "validate_runtime", return_value=None),
            patch.object(stage, "write_link_sdk", return_value=root / "sdk"),
            patch.object(
                stage,
                "build_alsa_static",
                side_effect=lambda *arguments, **keywords: calls.append(arguments),
            ),
        )
        with patches[0], patches[1], patches[2], patches[3], patches[4], patches[5]:
            stage.cmd_stage(arguments)
    return calls


class AlsaGateTests(unittest.TestCase):
    def test_darwin_does_not_build_alsa(self):
        self.assertEqual(_stage_calls("aarch64-apple-darwin"), [])

    def test_linux_gnu_builds_alsa(self):
        calls = _stage_calls("x86_64-unknown-linux-gnu")
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0][1], "x86_64-unknown-linux-gnu")


if __name__ == "__main__":
    unittest.main()
