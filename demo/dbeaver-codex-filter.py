#!/usr/bin/env python3
"""Keep Codex's native terminal stream, removing only runtime metadata."""
from __future__ import annotations

import re
import sys

ANSI = re.compile(r"\033\[[0-?]*[ -/]*[@-~]")
PRIVATE_KEY_BEGIN = re.compile(r"-----BEGIN [^-]*PRIVATE KEY-----")
PRIVATE_KEY_END = re.compile(r"-----END [^-]*PRIVATE KEY-----")
IN_PRIVATE_KEY = False
SECRET_ASSIGNMENT = re.compile(
    r"""
    (?P<key>"?(?:password|passwd|secret|token|api[_-]?key|authorization)"?)
    \s*[:=]\s*
    (?:
        "(?:\\.|[^"\\])*"
        |'(?:\\.|[^'\\])*'
        |[^\s,;]+(?:\s+[^\s,;]+)*
    )
    """,
    re.IGNORECASE | re.VERBOSE,
)
PERSONAL_PATH = re.compile(r"/Users/[^\s\"']+")
METADATA_LINE = re.compile(r"^(?P<indent>\s*)(?P<field>workdir|approval|session id):", re.IGNORECASE)


def filter_line(line: str) -> str:
    global IN_PRIVATE_KEY

    plain = ANSI.sub("", line)
    metadata = METADATA_LINE.match(plain.strip())
    if metadata:
        return f"{metadata.group('indent')}{metadata.group('field')}: ****\n"

    begin = PRIVATE_KEY_BEGIN.search(plain)
    if IN_PRIVATE_KEY:
        if PRIVATE_KEY_END.search(plain):
            IN_PRIVATE_KEY = False
        return ""
    if begin:
        if not PRIVATE_KEY_END.search(plain, begin.end()):
            IN_PRIVATE_KEY = True
        return "[private-key-redacted]\n"
    line = SECRET_ASSIGNMENT.sub(
        lambda match: f"{match.group('key')}=[redacted]", line
    )
    line = line.replace("demo-password", "[db-password-redacted]")
    return PERSONAL_PATH.sub("[personal-path]", line)


for line in sys.stdin:
    output = filter_line(line)
    if output:
        sys.stdout.write(output)
        sys.stdout.flush()
