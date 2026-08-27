#!/usr/bin/env python3
"""Do not downgrade a package manager while recovering an older release."""
from pathlib import Path
import re
import sys


def check(formula, requested):
    new = re.fullmatch(r"v(\d+)\.(\d+)\.(\d+)", requested)
    if not new:
        raise ValueError("Package managers require a stable semver tag")
    if not formula.exists():
        return
    versions = re.findall(r"releases/download/v(\d+)\.(\d+)\.(\d+)/", formula.read_text())
    if not versions:
        raise ValueError("Cannot determine the current formula version; refusing to overwrite")
    if max(tuple(map(int, version)) for version in versions) > tuple(map(int, new.groups())):
        raise ValueError("Refusing to downgrade an already published Homebrew version")


if __name__ == "__main__":
    check(Path(sys.argv[1]), sys.argv[2])
