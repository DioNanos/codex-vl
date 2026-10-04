#!/usr/bin/env python3
"""Tests for the npm pack --json output normalization (npm <=11 vs npm 12)."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from build_npm_package import normalize_npm_pack_output  # noqa: E402

ENTRY = {
    "filename": "mmmbuto-codex-vl-0.159.3.tgz",
    "name": "@mmmbuto/codex-vl",
    "version": "0.159.3",
}


class NormalizeNpmPackOutputTest(unittest.TestCase):
    def test_npm_le_11_list_shape(self):
        parsed = [ENTRY]
        normalized = normalize_npm_pack_output(parsed)
        self.assertIsInstance(normalized, list)
        self.assertEqual(normalized[0]["filename"], ENTRY["filename"])

    def test_npm_12_keyed_object_shape(self):
        parsed = {"@mmmbuto/codex-vl": ENTRY}
        normalized = normalize_npm_pack_output(parsed)
        self.assertIsInstance(normalized, list)
        self.assertEqual(normalized[0]["filename"], ENTRY["filename"])

    def test_empty_list_stays_empty(self):
        self.assertEqual(normalize_npm_pack_output([]), [])

    def test_unexpected_shape_raises(self):
        with self.assertRaises(RuntimeError):
            normalize_npm_pack_output("not-a-shape")


if __name__ == "__main__":
    unittest.main()
