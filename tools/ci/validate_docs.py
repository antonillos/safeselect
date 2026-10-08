#!/usr/bin/env python3
"""Fail fast on basic Markdown hygiene and repository-local links."""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LINK = re.compile(r"(?<!!)\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")


def markdown_files() -> list[Path]:
    # Use the Git index, not the working-tree directory inventory: generated
    # caches and untracked dependencies are not repository documentation.
    # -z preserves filenames containing whitespace or newlines.
    tracked = subprocess.check_output(
        ["git", "ls-files", "--cached", "-z"], cwd=ROOT
    )
    return sorted({
        ROOT / filename.decode("utf-8", errors="surrogateescape")
        for filename in tracked.split(b"\0")
        if filename.endswith(b".md")
    })


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
