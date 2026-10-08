import unittest
from pathlib import Path


class IntegrationArtifactNamesTest(unittest.TestCase):
    def test_log_names_include_postgres_matrix_version(self):
        workflow = (Path(__file__).resolve().parents[2] / ".github/workflows/integration-tests.yml").read_text()
        self.assertIn("name: integration-test-logs-pg${{ matrix.postgres-version }}", workflow)
        names = [f"integration-test-logs-pg{v}" for v in (15, 16, 17, 18)]
        self.assertEqual(len(names), len(set(names)))

    def test_startup_failure_collects_bounded_logs_before_cleanup(self):
        workflow = (Path(__file__).resolve().parents[2] / ".github/workflows/integration-tests.yml").read_text()
        self.assertIn("id: services", workflow)
        self.assertIn("if: failure() && steps.services.outcome == 'failure'", workflow)
        self.assertIn("logs --no-color --tail=100 postgres mongodb", workflow)
        self.assertLess(workflow.index("Diagnose integration startup failure"),
                        workflow.index("Stop integration services"))


if __name__ == "__main__":
    unittest.main()
