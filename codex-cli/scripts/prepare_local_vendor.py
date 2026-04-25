#!/usr/bin/env python3
"""Prepare a local vendor payload for Codex npm platform packages."""

from __future__ import annotations

import argparse
import importlib.util
import shutil
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
INSTALL_NATIVE_DEPS = SCRIPT_DIR / "install_native_deps.py"

_SPEC = importlib.util.spec_from_file_location("codex_install_native_deps", INSTALL_NATIVE_DEPS)
if _SPEC is None or _SPEC.loader is None:
    raise RuntimeError(f"Unable to load module from {INSTALL_NATIVE_DEPS}")
_INSTALL_NATIVE_DEPS = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_INSTALL_NATIVE_DEPS)

fetch_rg = getattr(_INSTALL_NATIVE_DEPS, "fetch_rg")
RG_MANIFEST = getattr(_INSTALL_NATIVE_DEPS, "RG_MANIFEST")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--vendor-root",
        type=Path,
        required=True,
        help="Directory where vendor/<target>/... will be created.",
    )
    parser.add_argument(
        "--target",
        required=True,
        help="Rust target triple for the package payload.",
    )
    parser.add_argument(
        "--codex-binary",
        type=Path,
        required=True,
        help="Path to the locally built codex binary for the target.",
    )
    parser.add_argument(
        "--include-rg",
        action="store_true",
        help="Fetch ripgrep for this target using the checked-in DotSlash manifest.",
    )
    return parser.parse_args()


def ensure_executable(path: Path) -> None:
    mode = path.stat().st_mode
    path.chmod(mode | 0o755)


def main() -> int:
    args = parse_args()

    vendor_root = args.vendor_root.resolve()
    target_root = vendor_root / args.target
    codex_dest_dir = target_root / "codex"
    codex_dest_dir.mkdir(parents=True, exist_ok=True)

    codex_binary = args.codex_binary.resolve()
    if not codex_binary.exists():
        raise FileNotFoundError(f"codex binary not found: {codex_binary}")

    codex_dest = codex_dest_dir / "codex"
    shutil.copy2(codex_binary, codex_dest)
    ensure_executable(codex_dest)

    if args.include_rg:
        fetch_rg(vendor_root, [args.target], manifest_path=RG_MANIFEST)

    print(f"Prepared vendor payload for {args.target} in {vendor_root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
