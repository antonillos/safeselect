#!/usr/bin/env python3
"""Regression tests for the public Codex recording formatter."""
from __future__ import annotations

import json
import re
import subprocess
import sys
import unittest
from pathlib import Path


FORMATTER = Path(__file__).with_name("dbeaver-codex-format.py")
ANSI = re.compile(r"\033\[[0-9;]*m")


class FormatterTests(unittest.TestCase):
    def render_message(self, message: str) -> str:
        event = {
            "type": "item.completed",
            "item": {"type": "agent_message", "text": message},
        }
        result = subprocess.run(
            [sys.executable, str(FORMATTER)],
            input=json.dumps(event) + "\n",
            text=True,
            capture_output=True,
            check=True,
        )
        return ANSI.sub("", result.stdout)

    def test_redacts_json_quoted_secret_values(self) -> None:
        rendered = self.render_message('payload: "password": "top-secret"')
        self.assertIn('"password"=[redacted]', rendered)
        self.assertNotIn("top-secret", rendered)

    def test_redacts_complete_unquoted_secret_values(self) -> None:
        rendered = self.render_message("password=multi word secret")
        self.assertIn("password=[redacted]", rendered)
        self.assertNotIn("multi word secret", rendered)

    def test_does_not_invent_missing_reasoning_summary(self) -> None:
        event = {
            "type": "item.completed",
            "item": {"type": "reasoning", "summary": []},
        }
        result = subprocess.run(
            [sys.executable, str(FORMATTER)],
            input=json.dumps(event) + "\n",
            text=True,
            capture_output=True,
            check=True,
        )
        self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
