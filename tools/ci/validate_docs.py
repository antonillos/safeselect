#!/usr/bin/env python3
"""Fail fast on basic Markdown hygiene and repository-local links."""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LINK = re.compile(r"(?<!!)\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")


def markdown_files() -> list[Path]:
    # Website dependencies/builds are not repository documentation. Prune them
    # before traversal, rather than validating thousands of third-party READMEs.
    import os

    excluded = {".git", "node_modules", ".next", ".vinext", ".wrangler", ".build", "target", "out", "dist"}
    files = []
    for directory, children, names in os.walk(ROOT):
        children[:] = [name for name in children if name not in excluded]
        files.extend(Path(directory) / name for name in names if name.endswith(".md"))
    return sorted(files)


def local_target(source: Path, raw: str) -> Path | None:
    target = raw.split("#", 1)[0].split("?", 1)[0]
    if not target or target.startswith(("http://", "https://", "mailto:", "tel:")):
        return None
    return ROOT / target.lstrip("/") if target.startswith("/") else source.parent / target


def main() -> int:
    errors: list[str] = []
    for path in markdown_files():
        text = path.read_text(encoding="utf-8")
        relative = path.relative_to(ROOT)
        if not text.endswith("\n"):
            errors.append(f"{relative}: file must end with a newline")
        for line_number, line in enumerate(text.splitlines(), start=1):
            if line.rstrip(" \t") != line:
                errors.append(f"{relative}:{line_number}: trailing whitespace")
        for match in LINK.finditer(text):
            target = local_target(path, match.group(1))
            if target is not None and not target.exists():
                errors.append(f"{relative}: missing local link target {match.group(1)!r}")
    if errors:
        print("Markdown validation failed:", *errors, sep="\n", file=sys.stderr)
        return 1
    print("Markdown formatting and repository-local links are valid.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
