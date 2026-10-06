#!/usr/bin/env python3

"""Verify codex-tui does not depend on or import codex-core directly."""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
TUI_ROOT = ROOT / "codex-rs" / "tui"
TUI_MANIFEST = TUI_ROOT / "Cargo.toml"
FORBIDDEN_PACKAGE = "codex-core"
FORBIDDEN_SOURCE_PATTERNS = (
    re.compile(r"\bcodex_core::"),
    re.compile(r"\buse\s+codex_core\b"),
    re.compile(r"\bextern\s+crate\s+codex_core\b"),
)
# fork: the Vivling background runtime still links codex-core directly; tracked for migration behind legacy_core
ALLOWED_MANIFEST_CORE_DEP = {
    "codex-rs/tui/Cargo.toml",
}
# fork: the Vivling background runtime still links codex-core directly; tracked for migration behind legacy_core
ALLOWED_SOURCE_FILES = {
    "codex-rs/tui/src/app/vivling_background.rs",
}


def main() -> int:
    failures = []
    failures.extend(manifest_failures())
    failures.extend(source_failures())

    if not failures:
        return 0

    print("codex-tui must not depend on or import codex-core directly.")
    print(
        "Use the app-server protocol/client boundary instead; temporary embedded "
        "startup gaps belong behind codex_app_server_client::legacy_core."
    )
    print()
    for failure in failures:
        print(f"- {failure}")

    return 1


def manifest_failures() -> list[str]:
    failures = []
    manifest_rel = relative_path(TUI_MANIFEST)
    if TUI_MANIFEST.is_file():
        manifest = tomllib.loads(TUI_MANIFEST.read_text())
        for section_name, dependencies in dependency_sections(manifest):
            if FORBIDDEN_PACKAGE not in dependencies:
                continue
            if manifest_rel in ALLOWED_MANIFEST_CORE_DEP:
                continue
            failures.append(
                f"{manifest_rel} declares `{FORBIDDEN_PACKAGE}` "
                f"in `[{section_name}]`"
            )
    elif manifest_rel not in ALLOWED_MANIFEST_CORE_DEP:
        failures.append(f"{manifest_rel} is missing")

    for allowed in sorted(ALLOWED_MANIFEST_CORE_DEP):
        path = ROOT / allowed
        if not path.is_file():
            failures.append(
                f"stale `ALLOWED_MANIFEST_CORE_DEP` entry `{allowed}` does not exist"
            )
        elif not manifest_declares_core(path):
            failures.append(
                "stale `ALLOWED_MANIFEST_CORE_DEP` entry "
                f"`{allowed}` no longer declares `{FORBIDDEN_PACKAGE}`"
            )
    return failures


def manifest_declares_core(path: Path) -> bool:
    manifest = tomllib.loads(path.read_text())
    return any(
        FORBIDDEN_PACKAGE in dependencies
        for _section_name, dependencies in dependency_sections(manifest)
    )


def dependency_sections(manifest: dict) -> list[tuple[str, dict]]:
    sections: list[tuple[str, dict]] = []
    for section_name in ("dependencies", "dev-dependencies", "build-dependencies"):
        dependencies = manifest.get(section_name)
        if isinstance(dependencies, dict):
            sections.append((section_name, dependencies))

    for target_name, target in manifest.get("target", {}).items():
        if not isinstance(target, dict):
            continue
        for section_name in ("dependencies", "dev-dependencies", "build-dependencies"):
            dependencies = target.get(section_name)
            if isinstance(dependencies, dict):
                sections.append((f"target.{target_name}.{section_name}", dependencies))

    return sections


def source_failures() -> list[str]:
    failures = []
    imported: set[str] = set()
    for path in sorted(TUI_ROOT.glob("**/*.rs")):
        rel = relative_path(path)
        for line_number, line in enumerate(path.read_text().splitlines(), start=1):
            if not line_imports_core(line):
                continue
            if rel in ALLOWED_SOURCE_FILES:
                imported.add(rel)
                continue
            failures.append(f"{rel}:{line_number} imports `codex_core`")

    for allowed in sorted(ALLOWED_SOURCE_FILES):
        path = ROOT / allowed
        if not path.is_file():
            failures.append(
                f"stale `ALLOWED_SOURCE_FILES` entry `{allowed}` does not exist"
            )
        elif allowed not in imported and not file_imports_core(path):
            failures.append(
                "stale `ALLOWED_SOURCE_FILES` entry "
                f"`{allowed}` no longer imports `codex_core`"
            )
    return failures


def line_imports_core(line: str) -> bool:
    return any(pattern.search(line) for pattern in FORBIDDEN_SOURCE_PATTERNS)


def file_imports_core(path: Path) -> bool:
    return any(line_imports_core(line) for line in path.read_text().splitlines())


def relative_path(path: Path) -> str:
    return str(path.relative_to(ROOT))


if __name__ == "__main__":
    sys.exit(main())
