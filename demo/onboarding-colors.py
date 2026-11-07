#!/usr/bin/env python3
"""Add ANSI colors to the recording while retaining the complete source stream."""

import re
import sys
from pathlib import Path


ANSI = re.compile(rb"\x1b\[[0-?]*[ -/]*[@-~]")
RESET = b"\x1b[0m"


def colorize(line: bytes) -> bytes:
    visible = ANSI.sub(b"", line).lstrip()
    if visible.startswith(b"> "):
        color = b"\x1b[95m"
    elif visible.startswith("⚙".encode()):
        color = b"\x1b[96m"
    elif visible.startswith((b"* ", b"Thinking:")):
        color = b"\x1b[94m"
    else:
        return line
    return color + ANSI.sub(lambda match: match.group() + color, line) + RESET


def main() -> None:
    with Path(sys.argv[1]).open("wb") as original:
        for line in sys.stdin.buffer:
            original.write(line)
            original.flush()
            sys.stdout.buffer.write(colorize(line))
            sys.stdout.buffer.flush()


if __name__ == "__main__":
    main()
