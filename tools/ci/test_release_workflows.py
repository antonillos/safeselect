"""Regression contracts complement actionlint's YAML/expression validation."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]


class WorkflowTests(unittest.TestCase):
    def setUp(self):
        self.release = (ROOT / ".github/workflows/release.yml").read_text()
        self.jobs = dict(re.findall(r"^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)",
                                    self.release.split("jobs:\n", 1)[1], re.M | re.S))

    def test_builds_stage_artifacts_without_publication_permission(self):
        build = self.jobs["build"]
        self.assertIn("needs: [validate-version, integration-tests]", build)
        self.assertIn("actions/upload-artifact@v7", build)
        self.assertNotIn("gh release upload", build)
        self.assertNotIn("contents: write", build)
        self.assertIn("permissions:\n  contents: read", self.release)

    def test_publish_requires_entire_matrix_and_downloads_artifacts(self):
        publish = self.jobs["publish-release"]
        self.assertIn("needs: [validate-version, integration-tests, build]", publish)
        self.assertIn("actions/download-artifact@v8", publish)
        self.assertIn("release.py publish", publish)
        self.assertNotIn("always()", publish)

    def test_distribution_waits_for_verified_public_release(self):
        for name in ("publish-package-managers", "publish-mcp-registry"):
            self.assertRegex(self.jobs[name], r"needs: \[[^\n]*publish-release")
            self.assertIn("outputs.draft != 'true'", self.jobs[name])

    def test_existing_release_not_blindly_skipped_deleted_or_overwritten(self):
        self.assertNotIn("outputs.exists", self.release)
        self.assertNotIn("gh release delete", self.release)
        self.assertNotIn("--clobber", self.release)
        self.assertIn("cancel-in-progress: false", self.release)

    def test_all_makevn_installs_use_local_pinned_action(self):
        for name in ("release", "prepare-release", "integration-tests", "verify"):
            text = (ROOT / f".github/workflows/{name}.yml").read_text()
            self.assertNotIn("makevn/main/packaging/install", text)
            self.assertIn("/.github/actions/setup-makevn", text)

    def test_makevn_action_configures_mirror_and_workflows_cache_dependencies(self):
        action = (ROOT / ".github/actions/setup-makevn/action.yml").read_text()
        self.assertIn(
            "https://maven-central.storage-download.googleapis.com/maven2/", action
        )
        self.assertIn("<mirrorOf>central</mirrorOf>", action)
        self.assertIn("MAVEN_ARGS", action)
        for name in ("release", "prepare-release", "integration-tests", "verify"):
            text = (ROOT / f".github/workflows/{name}.yml").read_text()
            self.assertIn("cache: maven", text)
            self.assertIn("cache-dependency-path: '**/pom.xml'", text)

    def test_older_source_uses_current_tooling_and_source_sha(self):
        self.assertIn("path: release-source", self.release)
        self.assertIn("outputs.target-ref", self.jobs["build"])
        for name in ("integration-tests", "prepare-release"):
            text = (ROOT / f".github/workflows/{name}.yml").read_text()
            self.assertIn("ref: ${{ github.sha }}", text)
            self.assertIn("path: .ci-tools", text)

    def test_package_manager_recovery_has_same_verification_and_no_silent_skips(self):
        package = (ROOT / ".github/workflows/publish-package-managers.yml").read_text()
        self.assertIn("workflow_call:", package)
        self.assertIn("release.py check-public", package)
        self.assertNotIn("|| true", package)
        self.assertNotIn("Skipping", package)
        self.assertIn("check_package_version.py", package)


if __name__ == "__main__":
    unittest.main()
