#!/usr/bin/env python3

"""Verify codex-tui has no unreviewed direct codex-core boundary."""

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
ALLOWED_MANIFEST_DEPENDENCIES = {
    ("dependencies", "codex-core"),
}
ALLOWED_SOURCE_IMPORTS = {
    (
        "codex-rs/tui/src/app/vivling_background.rs",
        "use codex_core::CodexResponsesMetadata;",
    ),
    ("codex-rs/tui/src/app/vivling_background.rs", "use codex_core::Prompt;"),
    (
        "codex-rs/tui/src/app/vivling_background.rs",
        "use codex_core::ResponseEvent;",
    ),
    (
        "codex-rs/tui/src/app/vivling_background.rs",
        "use codex_core::build_background_model_client;",
    ),
    (
        "codex-rs/tui/src/app/vivling_background.rs",
        "use codex_core::content_items_to_text;",
    ),
}


def main() -> int:
    failures = []
    failures.extend(manifest_failures())
    failures.extend(source_failures())

    if not failures:
        return 0

    print("codex-tui must not add unreviewed codex-core dependencies or imports.")
    print(
        "Use the app-server protocol/client boundary instead; temporary embedded "
        "startup gaps belong behind codex_app_server_client::legacy_core."
    )
    print()
    for failure in failures:
        print(f"- {failure}")

    return 1


def manifest_failures() -> list[str]:
    manifest = tomllib.loads(TUI_MANIFEST.read_text())
    failures = []
    used_exceptions = set()
    for section_name, dependencies in dependency_sections(manifest):
        if FORBIDDEN_PACKAGE in dependencies:
            exception = (section_name, FORBIDDEN_PACKAGE)
            if exception in ALLOWED_MANIFEST_DEPENDENCIES:
                used_exceptions.add(exception)
            else:
                failures.append(
                    f"{relative_path(TUI_MANIFEST)} declares `{FORBIDDEN_PACKAGE}` "
                    f"in `[{section_name}]`"
                )
    for section_name, dependency in sorted(
        ALLOWED_MANIFEST_DEPENDENCIES - used_exceptions
    ):
        failures.append(
            "remove stale direct-dependency exception for "
            f"`[{section_name}].{dependency}`"
        )
    return failures


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
    used_exceptions = set()
    for path in sorted(TUI_ROOT.glob("**/*.rs")):
        text = path.read_text()
        for line_number, line in enumerate(text.splitlines(), start=1):
            if any(pattern.search(line) for pattern in FORBIDDEN_SOURCE_PATTERNS):
                exception = (relative_path(path), line.strip())
                if exception in ALLOWED_SOURCE_IMPORTS:
                    used_exceptions.add(exception)
                else:
                    failures.append(
                        f"{relative_path(path)}:{line_number} imports `codex_core`"
                    )
    for path, source_line in sorted(ALLOWED_SOURCE_IMPORTS - used_exceptions):
        failures.append(
            f"remove stale direct-import exception for {path}: `{source_line}`"
        )
    return failures


def relative_path(path: Path) -> str:
    return str(path.relative_to(ROOT))


if __name__ == "__main__":
    sys.exit(main())
