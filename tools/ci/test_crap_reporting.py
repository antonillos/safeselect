"""Exercise the real CRAP wrapper/reporters with synthetic analyzer output.

No Rust/Java compilation, coverage execution, or project target cleanup is needed:
each test copies the tools to a disposable root and stubs only the two analyzers.
"""

import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class CrapReportingTests(unittest.TestCase):
    def setUp(self):
        self.assertIsNotNone(shutil.which("jq"), "jq is required to test CRAP reporting")
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.tools = self.root / "tools/crap"
        shutil.copytree(ROOT / "tools/crap", self.tools)
        (self.root / "fixtures").mkdir()
        self.out = self.root / "target/crap"
        for language in ("rust", "java"):
            analyzer = self.tools / f"{language}-crap.sh"
            analyzer.write_text(
                '#!/usr/bin/env bash\nset -euo pipefail\n'
                'ROOT="$(cd "$(dirname "$0")/../.." && pwd)"\n'
                f'cp "$ROOT/fixtures/{language}.json" '
                f'"$ROOT/target/crap/{language}-report.json"\n'
            )
            analyzer.chmod(0o755)

    def fixtures(self, rust_scores, java_scores=()):
        rust = [{"file": "src/example.rs", "line": i + 1,
                 "function": f"rust_{i}", "cyclomatic": 2,
                 "coverage": None if score is None else 50, "crap": score}
                for i, score in enumerate(rust_scores)]
        java = [{"file": "sidecar/Example.java", "line": i + 1,
                 "end_line": i + 1, "class": "Example", "method": f"java_{i}",
                 "complexity": 2, "coverage_percent": None if score is None else 50,
                 "crap": score, "status": "missing-coverage" if score is None else "measured"}
                for i, score in enumerate(java_scores)]
        for name, entries in (("rust", rust), ("java", java)):
            (self.root / f"fixtures/{name}.json").write_text(json.dumps({"entries": entries}))

    def run_wrapper(self, *args):
        return subprocess.run(["bash", str(self.tools / "run.sh"), "--summary", *args],
                              cwd=self.root, capture_output=True, text=True, timeout=20)

    def report(self):
        return json.loads((self.out / "report.json").read_text())

    def test_standalone_report_does_not_impose_a_gate(self):
        self.fixtures([9] * 81)
        result = self.run_wrapper()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.report()["gate"], {"mode": "report-only"})
        self.assertIn("REPORT-ONLY — no warning-count gate configured", (self.out / "report.md").read_text())
        self.assertIn("Gate: REPORT-ONLY", (self.out / "summary.txt").read_text())

    def test_existing_80_warning_boundary_and_cross_language_count(self):
        for count, expected_code, status in ((80, 0, "passed"), (81, 1, "failed")):
            with self.subTest(count=count):
                self.fixtures([9] * 40, [9] * (count - 40))
                result = self.run_wrapper("--ratchet", "80")
                self.assertEqual(result.returncode, expected_code, result.stderr)
                self.assertEqual(self.report()["gate"], {
                    "mode": "ratchet", "max_warnings": 80, "warnings": count, "status": status})
                self.assertEqual(self.report()["threshold"], 8)
                markdown = (self.out / "report.md").read_text()
                self.assertIn(f"**Status:** {status.upper()} — {count}/80 warnings", markdown)
                self.assertNotIn("REPORT-ONLY", markdown)
                self.assertIn("--summary --ratchet 80", markdown)
                self.assertIn(f"Gate: {status.upper()} ({count}/80 warnings)",
                              (self.out / "summary.txt").read_text())
                # Failure still leaves readable artifacts for CI upload.
                self.assertEqual(len(json.loads((self.out / "report.sarif").read_text())
                                     ["runs"][0]["results"]), count)
                self.assertEqual(json.loads((self.out / "badge.json").read_text())
                                 ["message"], f"{count} warnings")

    def test_score_threshold_is_strict_and_missing_coverage_is_unknown(self):
        self.fixtures([8, None], [8.01, None])
        result = self.run_wrapper("--ratchet", "1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.report()["gate"]["warnings"], 1)
        self.assertEqual(self.report()["diagnostics"]["missing_coverage"], 2)
        self.assertEqual(sum(entry["crap"] is None for entry in self.report()["entries"]), 2)

    def test_score_of_80_is_one_warning_not_a_count_limit(self):
        self.fixtures([80])
        result = self.run_wrapper("--ratchet", "1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.report()["gate"]["warnings"], 1)

    def test_zero_limit_is_not_treated_as_report_only(self):
        for scores, expected_code in (([], 0), ([9], 1)):
            with self.subTest(scores=scores):
                self.fixtures(scores)
                result = self.run_wrapper("--ratchet", "0")
                self.assertEqual(result.returncode, expected_code, result.stderr)
                self.assertEqual(self.report()["gate"]["mode"], "ratchet")
                self.assertEqual(self.report()["gate"]["max_warnings"], 0)

    def test_invalid_or_missing_limit_rejected_before_analyzers(self):
        for args in (("--ratchet",), ("--ratchet", "-1"), ("--ratchet", "1.5"),
                     ("--ratchet", "no")):
            with self.subTest(args=args):
                result = self.run_wrapper(*args)
                self.assertEqual(result.returncode, 2)
                self.assertFalse(self.out.exists())

    def test_report_entrypoint_preserves_custom_score_threshold(self):
        self.fixtures([5, 6])
        result = subprocess.run(
            ["bash", str(self.tools / "report.sh"), str(self.root / "fixtures/rust.json"),
             str(self.root / "fixtures/java.json"), "5", "1"],
            capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.report()["threshold"], 5)
        self.assertEqual(self.report()["gate"]["warnings"], 1)
        self.assertEqual(self.report()["gate"]["status"], "passed")

    def test_invalid_report_limit_does_not_write_artifacts(self):
        result = subprocess.run(
            ["bash", str(self.tools / "report.sh"), "unused", "unused", "8", "invalid"],
            capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 2)
        self.assertFalse(self.out.exists())


if __name__ == "__main__":
    unittest.main()
