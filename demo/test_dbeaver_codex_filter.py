#!/usr/bin/env python3
"""Regression tests for the surgical native Codex stream filter."""
from __future__ import annotations

import subprocess
import sys
import unittest
from pathlib import Path


FILTER = Path(__file__).with_name("dbeaver-codex-filter.py")


class FilterTests(unittest.TestCase):
    def filter(self, text: str) -> str:
        result = subprocess.run(
            [sys.executable, str(FILTER)],
            input=text,
            text=True,
            capture_output=True,
            check=True,
        )
        return result.stdout

    def test_removes_only_selected_runtime_metadata(self) -> None:
        output = self.filter(
            "workdir: /private/tmp/demo\n"
            "approval: on-request\n"
            "session id: abc123\n"
            "model: gpt-5.6-luna\n"
            "User\n"
            "Please inspect the staging database.\n"
        )
        self.assertEqual(
            output,
            "workdir: ****\napproval: ****\nsession id: ****\n"
            "model: gpt-5.6-luna\nUser\nPlease inspect the staging database.\n",
        )

    def test_keeps_native_mcp_and_reasoning_lines(self) -> None:
        output = self.filter(
            "**Identifying database tools**\n"
            "mcp: safeselect-staging/check started\n"
            "mcp: safeselect-staging/check completed\n"
        )
        self.assertEqual(output, "**Identifying database tools**\n"
                             "mcp: safeselect-staging/check started\n"
                             "mcp: safeselect-staging/check completed\n")


if __name__ == "__main__":
    unittest.main()
