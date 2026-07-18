#!/usr/bin/env python3

import hashlib
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from codex_package.ripgrep import fetch_rg
from codex_package.targets import TARGET_SPECS


class FetchRipgrepTest(unittest.TestCase):
    def test_fetches_digest_verified_target_artifact(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive = create_archive(root, b"real ripgrep")
            manifest = create_manifest(root, archive)

            with patch(
                "codex_package.dotslash.default_cache_root",
                return_value=root / "cache",
            ):
                rg_bin = fetch_rg(
                    TARGET_SPECS["x86_64-unknown-linux-musl"],
                    manifest_path=manifest,
                )

            self.assertEqual(rg_bin.read_bytes(), b"real ripgrep")

    def test_rejects_artifact_with_wrong_digest(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive = create_archive(root, b"tampered ripgrep")
            manifest = create_manifest(root, archive, digest="0" * 64)

            with patch(
                "codex_package.dotslash.default_cache_root",
                return_value=root / "cache",
            ):
                with self.assertRaisesRegex(RuntimeError, "has sha256"):
                    fetch_rg(
                        TARGET_SPECS["x86_64-unknown-linux-musl"],
                        manifest_path=manifest,
                    )


def create_archive(root: Path, contents: bytes) -> Path:
    source = root / "rg"
    source.write_bytes(contents)
    archive = root / "ripgrep.tar.gz"
    with tarfile.open(archive, "w:gz") as tar:
        tar.add(source, arcname="ripgrep/rg")
    return archive


def create_manifest(root: Path, archive: Path, *, digest: str | None = None) -> Path:
    manifest = root / "rg-manifest"
    manifest.write_text(
        json.dumps(
            {
                "platforms": {
                    "linux-x86_64": {
                        "size": archive.stat().st_size,
                        "hash": "sha256",
                        "digest": digest
                        or hashlib.sha256(archive.read_bytes()).hexdigest(),
                        "format": "tar.gz",
                        "path": "ripgrep/rg",
                        "providers": [{"url": archive.as_uri()}],
                    }
                }
            }
        ),
        encoding="utf-8",
    )
    return manifest


if __name__ == "__main__":
    unittest.main()
