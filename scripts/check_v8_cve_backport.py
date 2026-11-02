#!/usr/bin/env python3
"""Decide whether a rusty_v8 archive was built with the Array.sort backport.

`patches/v8_array_sort_elements_kind.patch` carries a static marker string
(`codex-vl-v8-array-sort-guard-v1`, emitted into the compiled object next to
the guard it identifies). This script looks for that marker inside a staged
`librusty_v8_*.a[.gz]` archive.

The V8 release workflows run it on every artifact they produce: an archive
without the marker cannot be published under a patched release tag, which is
the failure mode this check exists to prevent -- a tag that promises the
CVE-2026-85046 fix while the archive was built from unpatched sources.

No toolchain is required: the archive is decompressed if needed and scanned as
bytes. A gzip archive is never a false positive here, because the marker is
searched after decompression and a random payload cannot contain it.

Exit codes are kept distinct on purpose (mirroring check_v8_sandbox.py):
0 marker present, 1 marker absent, 2 the archive could not be examined. A
producer must read 2 as "not verified" and stop, never as "probably fine".
"""

from __future__ import annotations

import gzip
import sys
from pathlib import Path

MARKER = b"codex-vl-v8-array-sort-guard-v1"

EXIT_MARKER_PRESENT = 0
EXIT_MARKER_ABSENT = 1
EXIT_CANNOT_JUDGE = 2


class CannotJudge(Exception):
    """The archive could not be examined at all."""


def read_archive(path: Path) -> bytes:
    try:
        blob = path.read_bytes()
    except OSError as exc:
        raise CannotJudge(f"cannot read {path}: {exc}") from exc
    if blob[:2] == b"\x1f\x8b":
        try:
            return gzip.decompress(blob)
        except (OSError, EOFError, gzip.BadGzipFile) as exc:
            raise CannotJudge(f"cannot decompress {path}: {exc}") from exc
    return blob


def main() -> int:
    if len(sys.argv) != 2:
        raise CannotJudge("usage: check_v8_cve_backport.py <librusty_v8...a[.gz]>")

    path = Path(sys.argv[1])
    if not path.is_file():
        raise CannotJudge(f"{path} does not exist")

    blob = read_archive(path)
    if MARKER in blob:
        print(f"{path.name}: backport marker present")
        return EXIT_MARKER_PRESENT
    print(
        f"{path.name}: backport marker absent. This archive was not built with "
        "the Array.sort elements-kind backport; do not publish it under a "
        "patched release tag.",
        file=sys.stderr,
    )
    return EXIT_MARKER_ABSENT


if __name__ == "__main__":
    try:
        sys.exit(main())
    except CannotJudge as exc:
        print(f"cannot judge this archive: {exc}", file=sys.stderr)
        sys.exit(EXIT_CANNOT_JUDGE)
