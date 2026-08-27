import importlib.util
import re
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(f"{name}.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


site = load("validate_site")
docs = load("validate_docs")


class SiteValidationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for route in site.ROUTES:
            directory = self.root / route.strip("/")
            directory.mkdir(parents=True, exist_ok=True)
            url = f"{site.ORIGIN}{site.BASE}{route}"
            image = '<meta property="og:image" content="https://antonillos.github.io/safeselect/og.png"><meta name="twitter:image" content="https://antonillos.github.io/safeselect/og.png">' if route == "/" else ""
            (directory / "index.html").write_text(f'<html><head><title>Test</title><link rel="canonical" href="{url}"><meta property="og:url" content="{url}"><meta property="og:title" content="Test"><meta name="twitter:title" content="Test"><meta name="description" content="Description"><meta property="og:description" content="Description"><meta name="twitter:description" content="Description">{image}</head><body><h1 id="main">Test</h1><a href="#main">Main</a><a href="/safeselect/compare/">Compare</a></body></html>')
        (self.root / ".nojekyll").touch()
        (self.root / "og.png").write_bytes(b"test")
        (self.root / "sitemap.xml").write_text('<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">' + ''.join(f'<url><loc>{site.ORIGIN}{site.BASE}{r}</loc></url>' for r in site.ROUTES) + '</urlset>')

    def replace(self, old, new):
        path = self.root / "index.html"
        path.write_text(path.read_text().replace(old, new))

    def test_complete_static_site(self):
        site.validate(self.root)

    def test_missing_base_path(self):
        self.replace('href="/safeselect/compare/"', 'href="/compare/"')
        with self.assertRaisesRegex(AssertionError, "wrong base path"):
            site.validate(self.root)

    def test_missing_anchor(self):
        self.replace('href="#main"', 'href="#missing"')
        with self.assertRaisesRegex(AssertionError, "missing anchor"):
            site.validate(self.root)

    def test_no_javascript_dependency(self):
        self.replace('</body>', '<script src="/app.js"></script></body>')
        with self.assertRaisesRegex(AssertionError, "JavaScript"):
            site.validate(self.root)

    def test_wrong_canonical(self):
        self.replace('rel="canonical" href="https://antonillos.github.io/safeselect/"', 'rel="canonical" href="https://preview.example/"')
        with self.assertRaisesRegex(AssertionError, "canonical"):
            site.validate(self.root)

    def test_duplicate_ids(self):
        with self.assertRaisesRegex(ValueError, "Duplicate id"):
            site.Page('<h1 id="x">One</h1><p id="x">Two</p>')

    def test_docs_skip_generated_dependencies(self):
        (self.root / "docs").mkdir()
        (self.root / "docs/guide.md").write_text("# Guide\n")
        for directory in ("site/node_modules/pkg", "site/out", "sidecar/target"):
            path = self.root / directory
            path.mkdir(parents=True)
            (path / "README.md").write_text("Unreviewed dependency docs")
        with patch.object(docs, "ROOT", self.root):
            self.assertEqual(docs.markdown_files(), [self.root / "docs/guide.md"])


class WebsiteWorkflowTests(unittest.TestCase):
    def setUp(self):
        root = Path(__file__).resolve().parents[2]
        self.workflow = (root / ".github/workflows/verify.yml").read_text()
        self.jobs = dict(re.findall(r"^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)",
                                    self.workflow.split("jobs:\n", 1)[1], re.M | re.S))

    def test_website_runs_even_for_badge_refresh(self):
        self.assertNotIn("badge_only", self.jobs["website"])
        self.assertIn("npm ci --prefix site --ignore-scripts", self.jobs["website"])
        self.assertIn("validate_site.py", self.jobs["website"])

    def test_single_pages_artifact_combines_website_and_badge(self):
        self.assertEqual(self.workflow.count("uses: actions/upload-pages-artifact@"), 1)
        publish = self.jobs["deploy-crap-badge"]
        self.assertIn("needs: [crap-report, website]", publish)
        for artifact in ("website-pages", "pages-badge"):
            self.assertIn(f"name: {artifact}", publish)
        for file in ("index.html", "sitemap.xml", "crap-badge.json"):
            self.assertIn(f"test -s target/pages/{file}", publish)

    def test_verify_rejects_non_successful_website(self):
        verify = self.jobs["verify"]
        self.assertRegex(verify, r"needs: \[[^\n]*website")
        self.assertIn("WEBSITE_RESULT: ${{ needs.website.result }}", verify)
        script = verify.split("run: |\n", 1)[1]
        env = {name: "false" for name in re.findall(r'\$([A-Z_]+_REQUIRED)', script)}
        env.update({name: "skipped" for name in re.findall(r'\$([A-Z_]+_RESULT)', script)})
        for result in ("success", "failure", "cancelled", "skipped"):
            with self.subTest(result=result):
                run = subprocess.run(["/bin/bash", "-c", script],
                                     env={**env, "WEBSITE_RESULT": result}, capture_output=True)
                self.assertEqual(run.returncode, 0 if result == "success" else 1)


if __name__ == "__main__":
    unittest.main()
