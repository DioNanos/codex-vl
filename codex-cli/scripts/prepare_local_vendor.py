#!/usr/bin/env python3
"""Prepare a local vendor payload for Codex npm platform packages."""

from __future__ import annotations

import argparse
import shutil
from pathlib import Path
import sys


SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[1]
PACKAGE_SCRIPTS_ROOT = REPO_ROOT / "scripts"
sys.path.insert(0, str(PACKAGE_SCRIPTS_ROOT))

from codex_package.ripgrep import fetch_rg
from codex_package.targets import TARGET_SPECS


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
        choices=sorted(TARGET_SPECS),
        help="Rust target triple for the package payload.",
    )
    parser.add_argument(
        "--codex-binary",
        type=Path,
        required=True,
        help="Path to the locally built codex binary for the target.",
    )
    parser.add_argument(
        "--codex-exec-binary",
        type=Path,
        help="Path to the locally built codex-exec binary for the target.",
    )
    parser.add_argument(
        "--include-rg",
        action="store_true",
        help="Fetch ripgrep for this target using the checked-in DotSlash manifest.",
    )
    return parser.parse_args()


def ensure_executable(path: Path) -> None:
    path.chmod(0o755)


def install_ripgrep(vendor_root: Path, target: str) -> Path:
    spec = TARGET_SPECS[target]
    source = fetch_rg(spec)
    dest = vendor_root / target / "path" / spec.rg_name
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, dest)
    ensure_executable(dest)
    return dest


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

    if args.codex_exec_binary is not None:
        codex_exec_binary = args.codex_exec_binary.resolve()
        if not codex_exec_binary.exists():
            raise FileNotFoundError(f"codex-exec binary not found: {codex_exec_binary}")

        codex_exec_dest = codex_dest_dir / "codex-exec"
        shutil.copy2(codex_exec_binary, codex_exec_dest)
        ensure_executable(codex_exec_dest)

    if args.include_rg:
        rg_dest = install_ripgrep(vendor_root, args.target)
        print(f"Installed verified ripgrep at {rg_dest}")

    print(f"Prepared vendor payload for {args.target} in {vendor_root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
