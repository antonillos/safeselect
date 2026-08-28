import copy
from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from manifest_contract import ManifestError, load_manifest, validate_manifest
import update_lobehub_manifest as updater


def minimal():
    return {"identifier": "example-mcp", "name": "Example", "version": "1.2.3"}


class ContractTests(unittest.TestCase):
    def test_minimal_full_and_real_manifest(self):
        validate_manifest(minimal())
        full = {**minimal(), "description": "Example", "author": "example",
                "authorUrl": "https://github.com/example", "homepage": "https://example.com",
                "cloudEndpoint": "https://example.com/mcp", "category": "developer",
                "icon": "X", "tags": ["mcp"], "resources": [], "prompts": [],
                "localizations": [{"locale": "es-ES", "name": "Ejemplo", "tags": []}],
                "tools": [{"name": "inspect", "inputSchema": {"type": "object", "anyOf": [
                    {"properties": {}}, {"required": ["database"]}]},
                    "outputSchema": {"type": "object"}}]}
        before = copy.deepcopy(full)
        validate_manifest(full)
        self.assertEqual(before, full)
        root = Path(__file__).resolve().parents[2]
        validate_manifest(load_manifest((root / "lhm.plugin.json").read_text()))

    def test_invalid_fields(self):
        invalid = [None, [], "text", {}, {**minimal(), "identifier": "bad ID"}]
        for field in ("identifier", "name", "version"):
            for value in (None, "", " ", 42, True):
                invalid.append({**minimal(), field: value})
        for field in ("author", "category", "description", "icon"):
            invalid.append({**minimal(), field: 42})
        for field in ("tools", "resources", "prompts", "localizations"):
            for value in ({}, "array", ["object"]):
                invalid.append({**minimal(), field: value})
        invalid += [{**minimal(), "tags": [1]}, {**minimal(), "unexpected": "PRIVATE"}]
        for manifest in invalid:
            with self.subTest(manifest=manifest), self.assertRaises(ManifestError):
                validate_manifest(manifest)

    def test_invalid_urls_tools_localizations(self):
        for field in ("homepage", "authorUrl", "cloudEndpoint"):
            for value in ("relative", "https://", "https://bad host/", "https://[broken",
                          "https://example.com:wrong", "https://user:PRIVATE@example.com",
                          "https://example.com\n", 42):
                with self.subTest(field=field), self.assertRaises(ManifestError):
                    validate_manifest({**minimal(), field: value})
        valid = {"name": "inspect", "inputSchema": {"type": "object"}}
        for tools in ([{}], [valid, valid], [{**valid, "inputSchema": {"type": "array"}}],
                      [{**valid, "outputSchema": None}], [{**valid, "description": 7}]):
            with self.assertRaises(ManifestError):
                validate_manifest({**minimal(), "tools": tools})
        for locales in ([{}], [{"locale": "es", "tags": [5]}],
                        [{"locale": "es"}, {"locale": "es"}],
                        [{"locale": "es", "unknown": "PRIVATE"}]):
            with self.assertRaises(ManifestError):
                validate_manifest({**minimal(), "localizations": locales})

    def test_duplicate_keys_and_non_json_constants(self):
        for text in ('{"name":"one","name":"two"}', '{"tools":[{"name":1,"name":2}]}',
                     '{"value":NaN}', '{"value":Infinity}', '{"value":-Infinity}', '{bad'):
            with self.assertRaises(ManifestError):
                load_manifest(text)

    def test_invalid_manifest_stops_before_introspection_without_echoing_values(self):
        with tempfile.TemporaryDirectory() as tmp:
            file = Path(tmp) / "manifest.json"
            file.write_text(json.dumps({**minimal(), "homepage": "PRIVATE_SECRET"}))
            before = file.read_bytes(), file.stat().st_mtime_ns
            argv = ["script", "--binary", "unused", "--postgres-driver", "unused",
                    "--driver-sha256", "0" * 64, "--manifest", str(file)]
            for mode in ([], ["--check"]):
                output = io.StringIO()
                with patch("sys.argv", argv + mode), patch.object(updater, "generate") as generate:
                    with redirect_stdout(output):
                        self.assertEqual(updater.main(), 2)
                    generate.assert_not_called()
                self.assertNotIn("PRIVATE_SECRET", output.getvalue())
                self.assertIn("homepage", output.getvalue())
                self.assertEqual((file.read_bytes(), file.stat().st_mtime_ns), before)


if __name__ == "__main__":
    unittest.main()
