import contextlib
import importlib.util
import io
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


spec = importlib.util.spec_from_file_location(
    "validate_docs", Path(__file__).with_name("validate_docs.py")
)
docs = importlib.util.module_from_spec(spec)
spec.loader.exec_module(docs)


class SetupDocumentationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "docs").mkdir()
        self.readme = self.root / "README.md"
        self.guide = self.root / "docs/agents.md"
        self.readme.write_text("[Agent guide](docs/agents.md)\n", encoding="utf-8")
        self.guide.write_text("It enters setup mode automatically.\n", encoding="utf-8")
        self.root_patch = patch.object(docs, "ROOT", self.root)
        self.root_patch.start()
        self.addCleanup(self.root_patch.stop)

    def validate(self):
        output = io.StringIO()
        with patch.object(docs, "markdown_files", return_value=[self.readme, self.guide]):
            with contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                result = docs.main()
        return result, output.getvalue()

    def test_accepts_guidance_in_linked_agent_guide(self):
        self.assertEqual(self.validate()[0], 0)

    def test_accepts_existing_readme_guidance(self):
        self.readme.write_text("It enters setup mode automatically.\n", encoding="utf-8")
        self.guide.write_text("Agent integration.\n", encoding="utf-8")
        self.assertEqual(self.validate()[0], 0)

    def test_requires_setup_guidance(self):
        self.guide.write_text("Agent integration.\n", encoding="utf-8")
        result, output = self.validate()
        self.assertEqual(result, 1)
        self.assertIn("must document implicit setup mode", output)

    def test_requires_link_after_guidance_is_moved(self):
        self.readme.write_text("SafeSelect.\n", encoding="utf-8")
        result, output = self.validate()
        self.assertEqual(result, 1)
        self.assertIn("README.md must link to docs/agents.md", output)

    def test_accepts_link_with_anchor(self):
        self.readme.write_text("[Setup](docs/agents.md#first-run-setup)\n", encoding="utf-8")
        self.assertEqual(self.validate()[0], 0)

    def test_rejects_nonexistent_flag_in_either_document(self):
        for path in [self.readme, self.guide]:
            with self.subTest(path=path.name):
                original = path.read_text(encoding="utf-8")
                path.write_text(original + "safeselect serve --setup\n", encoding="utf-8")
                result, output = self.validate()
                self.assertEqual(result, 1)
                self.assertIn("non-existent serve --setup flag", output)
                path.write_text(original, encoding="utf-8")


if __name__ == "__main__":
    unittest.main()
