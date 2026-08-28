import copy
from contextlib import contextmanager
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import update_lobehub_manifest as updater


def tool(name="select"):
    return {"name": name, "description": updater.PREFIX + "inspect metadata",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": False}}


def prompt():
    return {"name": "read_only_database_debugging", "description": "safe guidance", "arguments": []}


def resource():
    return {"uri": "safeselect://guide/read-only-database-debugging",
            "name": "Read-only database debugging guide", "description": "safe guidance",
            "mimeType": "text/markdown"}


class ManifestTests(unittest.TestCase):
    def test_atomic_write_preserves_permissions(self):
        with tempfile.TemporaryDirectory() as tmp:
            file = Path(tmp) / "manifest.json"
            file.write_bytes(b"original")
            file.chmod(0o640)
            updater.atomic_write(file, b"replacement")
            self.assertEqual(file.read_bytes(), b"replacement")
            self.assertEqual(file.stat().st_mode & 0o777, 0o640)
            self.assertEqual(list(Path(tmp).iterdir()), [file])

    def test_atomic_write_failures_preserve_original_and_remove_temp(self):
        with tempfile.TemporaryDirectory() as tmp:
            file = Path(tmp) / "manifest.json"
            file.write_bytes(b"original")
            before = file.read_bytes(), file.stat().st_mtime_ns
            for operation in ("fsync", "replace"):
                with patch.object(updater.os, operation, side_effect=OSError("synthetic failure")):
                    with self.assertRaises(OSError):
                        updater.atomic_write(file, b"replacement")
                self.assertEqual((file.read_bytes(), file.stat().st_mtime_ns), before)
                self.assertEqual(list(Path(tmp).iterdir()), [file])
            original = tempfile.NamedTemporaryFile

            @contextmanager
            def partial_write(**kwargs):
                with original(**kwargs) as output:
                    real_write = output.write

                    def fail(payload):
                        real_write(payload[:3])
                        raise OSError("synthetic disk full")

                    with patch.object(output, "write", side_effect=fail):
                        yield output

            with patch.object(updater.tempfile, "NamedTemporaryFile", side_effect=partial_write):
                with self.assertRaises(OSError):
                    updater.atomic_write(file, b"replacement")
            self.assertEqual((file.read_bytes(), file.stat().st_mtime_ns), before)
            self.assertEqual(list(Path(tmp).iterdir()), [file])

    def test_ci_checks_fresh_binary_without_publishing(self):
        root = Path(__file__).resolve().parents[2]
        workflow = (root / ".github/workflows/verify.yml").read_text()
        step = workflow.split("      - name: Check LobeHub MCP capabilities against the current source\n")[1]
        step = step.split("\n  integration:")[0]
        self.assertIn("cargo build --locked", step)
        self.assertIn('${GITHUB_WORKSPACE}/target/debug/safeselect', step)
        self.assertIn("--check", step)
        self.assertIn("sha256sum --check", step)
        self.assertNotIn("plugin update", step)
        self.assertNotIn("git commit", step)
        self.assertIn("unittest discover -s tools/distribution", workflow)

    def test_deterministic_output_despite_tool_and_object_key_order(self):
        pg = {name: tool(name) for name in ("zeta", "alpha")}
        pg["alpha"]["inputSchema"]["prefixItems"] = [
            {"type": "string"}, {"type": "integer"}]

        def reverse_keys(value):
            if isinstance(value, dict):
                return {key: reverse_keys(value[key]) for key in reversed(value)}
            if isinstance(value, list):
                return [reverse_keys(item) for item in value]
            return value

        first = updater.merge_tools(pg, {})
        second = updater.merge_tools(reverse_keys(pg), {})
        self.assertEqual(json.dumps(first), json.dumps(second))
        self.assertEqual(first[0]["inputSchema"]["prefixItems"],
                         pg["alpha"]["inputSchema"]["prefixItems"])

    def test_backend_union_and_no_input_mutation(self):
        pg = {"select": tool(), "database_info": tool("database_info")}
        mongo = {"find_documents": tool("find_documents"), "database_info": tool("database_info")}
        original = copy.deepcopy(pg)
        result = updater.merge_tools(pg, mongo)
        self.assertEqual([t["name"] for t in result], ["database_info", "find_documents", "select"])
        self.assertEqual(result[1]["description"], "MongoDB only. inspect metadata")
        self.assertEqual(result[2]["inputSchema"], pg["select"]["inputSchema"])
        self.assertEqual(pg, original)

    def test_known_schema_variant(self):
        pg = tool("get_database_stats")
        mongo = copy.deepcopy(pg)
        mongo["inputSchema"] = {"type": "object", "required": ["database"]}
        result = updater.merge_tools({pg["name"]: pg}, {mongo["name"]: mongo})[0]
        self.assertEqual(result["inputSchema"]["anyOf"], [pg["inputSchema"], mongo["inputSchema"]])

    def test_unknown_difference_fails(self):
        pg, mongo = tool(), tool()
        mongo["description"] += " changed"
        with self.assertRaises(ValueError):
            updater.merge_tools({"select": pg}, {"select": mongo})

    def test_driver_mismatch_does_not_start_binary(self):
        with tempfile.TemporaryDirectory() as tmp:
            jar = Path(tmp) / "driver.jar"
            jar.write_bytes(b"synthetic fixture")
            with patch.object(updater, "capture") as capture:
                with self.assertRaises(ValueError):
                    updater.generate(Path("unused"), jar, "0" * 64, "1.2.3")
                capture.assert_not_called()

    def test_capture_only_metadata_and_isolated_environment(self):
        response = [
            {"jsonrpc": "2.0", "id": 1, "result": {"serverInfo": {"version": "1.2.3"}}},
            {"jsonrpc": "2.0", "id": 2, "result": {"tools": [tool()]}},
            {"jsonrpc": "2.0", "id": 3, "result": {"prompts": [prompt()]}},
            {"jsonrpc": "2.0", "id": 4, "result": {"resources": [resource()]}},
        ]
        with tempfile.TemporaryDirectory() as tmp, patch.object(updater.subprocess, "run") as run:
            run.return_value.returncode = 0
            run.return_value.stdout = "\n".join(map(json.dumps, response))
            updater.capture(Path("binary"), Path(tmp), "postgresql", "1.2.3")
            kwargs = run.call_args.kwargs
            methods = [json.loads(line)["method"] for line in kwargs["input"].splitlines()]
            self.assertEqual(methods, ["initialize", "notifications/initialized", "tools/list",
                                       "prompts/list", "resources/list"])
            self.assertEqual(set(kwargs["env"]), {"PATH", "HOME", "SAFESELECT_CONFIG_DIR",
                                                  "SAFESELECT_EXAMPLE_PASSWORD"})
            for mutate in ("version", "tool_cursor", "prompt_cursor", "resource_cursor", "duplicate"):
                bad = copy.deepcopy(response)
                if mutate == "version":
                    bad[0]["result"]["serverInfo"]["version"] = "9.0.0"
                elif mutate == "tool_cursor":
                    bad[1]["result"]["nextCursor"] = "more"
                elif mutate == "prompt_cursor":
                    bad[2]["result"]["nextCursor"] = "more"
                elif mutate == "resource_cursor":
                    bad[3]["result"]["nextCursor"] = "more"
                else:
                    bad[1]["result"]["tools"].append(tool())
                run.return_value.stdout = "\n".join(map(json.dumps, bad))
                with self.assertRaises(ValueError):
                    updater.capture(Path("binary"), Path(tmp), "postgresql", "1.2.3")

    def test_check_and_update_preserve_metadata(self):
        generated = {"tools": [tool()], "prompts": [prompt()], "resources": [resource()]}
        with tempfile.TemporaryDirectory() as tmp:
            file = Path(tmp) / "manifest.json"
            before = {"identifier": "example", "name": "Example", "version": "1.2.3",
                      "description": "SEO copy", "tags": ["mcp"]}
            file.write_text(json.dumps(before))
            argv = ["script", "--binary", str(file), "--postgres-driver", str(file),
                    "--driver-sha256", "0" * 64, "--manifest", str(file)]
            with patch.object(updater, "generate", return_value=generated):
                with patch("sys.argv", argv + ["--check"]):
                    self.assertEqual(updater.main(), 1)
                    self.assertEqual(json.loads(file.read_text()), before)
                with patch("sys.argv", argv):
                    self.assertEqual(updater.main(), 0)
                    self.assertEqual(json.loads(file.read_text()), {**before, **generated})
                saved = file.read_bytes()
                modified = file.stat().st_mtime_ns
                # Repeating an update is a true no-op, not just equal JSON.
                with patch("sys.argv", argv), patch.object(updater, "atomic_write") as write:
                    self.assertEqual(updater.main(), 0)
                    write.assert_not_called()
                with patch("sys.argv", argv + ["--check"]):
                    self.assertEqual(updater.main(), 0)
                self.assertEqual(file.read_bytes(), saved)
                self.assertEqual(file.stat().st_mtime_ns, modified)

    def test_failed_generation_does_not_modify_manifest(self):
        with tempfile.TemporaryDirectory() as tmp:
            file = Path(tmp) / "manifest.json"
            original = b'{"identifier":"example","name":"Example","version":"1.2.3", "description":"unchanged"}\n'
            file.write_bytes(original)
            modified = file.stat().st_mtime_ns
            argv = ["script", "--binary", str(file), "--postgres-driver", str(file),
                    "--driver-sha256", "0" * 64, "--manifest", str(file)]
            with patch.object(updater, "generate", side_effect=ValueError("bad capture")):
                with patch("sys.argv", argv):
                    self.assertEqual(updater.main(), 2)
            self.assertEqual(file.read_bytes(), original)
            self.assertEqual(file.stat().st_mtime_ns, modified)


if __name__ == "__main__":
    unittest.main()
