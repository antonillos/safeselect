#!/usr/bin/env python3
"""Keep Codex's native terminal stream, removing only runtime metadata."""
from __future__ import annotations

import re
import sys

ANSI = re.compile(r"\033\[[0-?]*[ -/]*[@-~]")
PRIVATE_KEY = re.compile(
    r"-----BEGIN [^-]*PRIVATE KEY-----.+?-----END [^-]*PRIVATE KEY-----",
    re.DOTALL,
)
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
    plain = ANSI.sub("", line).strip()
    metadata = METADATA_LINE.match(plain)
    if metadata:
        return f"{metadata.group('indent')}{metadata.group('field')}: ****\n"
    line = PRIVATE_KEY.sub("[private-key-redacted]", line)
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
