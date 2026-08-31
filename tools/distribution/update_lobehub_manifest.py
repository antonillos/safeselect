#!/usr/bin/env python3
"""Capture public MCP tool definitions without executing tools or publishing."""

import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import tempfile

from manifest_contract import ManifestError, load_manifest, validate_manifest


PREFIX = "SafeSelect database query MCP for project 'example-project' environment 'example': "
DATABASES = {
    "postgresql": 'driver = "postgresql"\nurl = "jdbc:postgresql://127.0.0.1:1/example"\n',
    "mongodb": 'kind = "document"\nvendor = "mongodb"\nurl = "mongodb://127.0.0.1:1/example"\n',
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def capture(binary, base, backend, version):
    project = base / "example-project"
    config = project / ".safeselect"
    (config / "environments").mkdir(parents=True, exist_ok=True)
    (config / "project.toml").write_text('version = 1\ndisplay_name = "example-project"\n')
    (config / "environments/example.toml").write_text(
        'version = 1\n[database]\n' + DATABASES[backend]
        + 'username = "example"\n[database.secret]\nsource = "env"\n'
        + 'variable = "SAFESELECT_EXAMPLE_PASSWORD"\n'
    )
    # Never inherit database credentials, Java hooks or the user's global config.
    env = {"PATH": os.defpath, "HOME": str(base),
           "SAFESELECT_CONFIG_DIR": str(base / "config"),
           "SAFESELECT_EXAMPLE_PASSWORD": "synthetic-not-a-real-password"}
    messages = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": {"name": "manifest-introspection", "version": "1.0"}}},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
        {"jsonrpc": "2.0", "id": 3, "method": "prompts/list"},
        {"jsonrpc": "2.0", "id": 4, "method": "resources/list"},
    ]
    result = subprocess.run(
        [str(binary), "serve", "--project", str(project), "--environment", "example"],
        input="".join(json.dumps(m) + "\n" for m in messages),
        text=True, capture_output=True, env=env, cwd=project, timeout=20,
    )
    require(result.returncode == 0, f"{backend}: introspection failed; manifest unchanged")
    replies = [json.loads(line) for line in result.stdout.splitlines()]
    responses = {}
    for reply in replies:
        if "id" not in reply:
            continue
        require(reply.get("jsonrpc") == "2.0" and "error" not in reply,
                f"{backend}: invalid MCP response")
        require(reply["id"] not in responses, f"{backend}: duplicate response")
        responses[reply["id"]] = reply["result"]
    require(responses[1]["serverInfo"]["version"] == version,
            "Binary version must match manifest version; rebuild/select the matching binary")
    require(not responses[2].get("nextCursor"),
            "Paginated tools/list is not supported; refusing an incomplete export")
    require(not responses[3].get("nextCursor"),
            "Paginated prompts/list is not supported; refusing an incomplete export")
    require(not responses[4].get("nextCursor"),
            "Paginated resources/list is not supported; refusing an incomplete export")
    tools = responses[2]["tools"]
    require(isinstance(tools, list) and tools, f"{backend}: no tools returned")
    indexed = {}
    for tool in tools:
        require(isinstance(tool, dict) and isinstance(tool.get("name"), str)
                and tool["name"] and isinstance(tool.get("description"), str)
                and isinstance(tool.get("inputSchema"), dict), "Invalid MCP tool definition")
        require(tool["name"] not in indexed, "Duplicate tool name")
        require(tool["description"].startswith(PREFIX),
                "Tool description context changed; review normalization before exporting")
        indexed[tool["name"]] = tool
    prompts = responses[3].get("prompts")
    resources = responses[4].get("resources")
    require(isinstance(prompts, list) and prompts, f"{backend}: no prompts returned")
    require(isinstance(resources, list) and resources, f"{backend}: no resources returned")
    for prompt in prompts:
        require(isinstance(prompt, dict) and isinstance(prompt.get("name"), str)
                and prompt["name"] and isinstance(prompt.get("description"), str),
                "Invalid MCP prompt definition")
    for resource in resources:
        require(isinstance(resource, dict) and isinstance(resource.get("uri"), str)
                and resource["uri"] and isinstance(resource.get("name"), str)
                and resource["name"] and isinstance(resource.get("description"), str)
                and isinstance(resource.get("mimeType"), str), "Invalid MCP resource definition")
    return indexed, prompts, resources


def merge_tools(postgres, mongo):
    """Preserve captured schemas; explicitly model the known backend variant."""
    merged = []
    for name in sorted(postgres.keys() | mongo.keys()):
        tool = copy.deepcopy(postgres.get(name, mongo.get(name)))
        if name in postgres and name in mongo and postgres[name] != mongo[name]:
            require(name == "get_database_stats", "New backend tool difference requires review")
            left, right = copy.deepcopy(postgres[name]), copy.deepcopy(mongo[name])
            for field in ("description", "inputSchema"):
                left.pop(field)
                right.pop(field)
            require(left == right, "Backend output/metadata difference requires review")
            tool["inputSchema"] = {"type": "object", "anyOf": [
                postgres[name]["inputSchema"], mongo[name]["inputSchema"]]}
            tool["description"] = (
                "PostgreSQL: " + postgres[name]["description"].removeprefix(PREFIX)
                + " MongoDB: " + mongo[name]["description"].removeprefix(PREFIX))
        else:
            scope = "PostgreSQL only. " if name not in mongo else (
                "MongoDB only. " if name not in postgres else "")
            tool["description"] = scope + tool["description"].removeprefix(PREFIX)
        merged.append(tool)
    # Canonicalize object keys recursively, but never reorder schema arrays:
    # positional arrays such as prefixItems can carry semantic meaning.
    return json.loads(json.dumps(merged, sort_keys=True, ensure_ascii=False))


def generate(binary, driver, expected_sha, version):
    require(re.fullmatch(r"[0-9a-fA-F]{64}", expected_sha), "Invalid driver SHA-256")
    with tempfile.TemporaryDirectory(prefix="safeselect-manifest-") as tmp:
        base = Path(tmp)
        drivers = base / "config/drivers"
        drivers.mkdir(parents=True)
        jar = drivers / "postgresql.jar"
        shutil.copyfile(driver, jar)
        require(hashlib.sha256(jar.read_bytes()).hexdigest() == expected_sha.lower(),
                "Driver SHA-256 mismatch; manifest unchanged")
        # JSON string escaping also produces valid TOML basic strings here.
        (drivers / "postgresql.toml").write_text(
            'version = 1\nvendor = "postgresql"\nclass = "org.postgresql.Driver"\n'
            + f'path = {json.dumps(str(jar))}\nsha256 = "{expected_sha.lower()}"\n')
        postgres_tools, postgres_prompts, postgres_resources = capture(
            binary, base, "postgresql", version)
        mongo_tools, mongo_prompts, mongo_resources = capture(binary, base, "mongodb", version)
        require(postgres_prompts == mongo_prompts,
                "Backend prompt definitions differ; refusing an ambiguous export")
        require(postgres_resources == mongo_resources,
                "Backend resource definitions differ; refusing an ambiguous export")
        generated = {
            "tools": merge_tools(postgres_tools, mongo_tools),
            "prompts": json.loads(json.dumps(postgres_prompts, sort_keys=True, ensure_ascii=False)),
            "resources": json.loads(json.dumps(postgres_resources, sort_keys=True, ensure_ascii=False)),
        }
        serialized = json.dumps(generated)
        require(all(value not in serialized for value in (
            str(base), "example-project", "synthetic-not-a-real-password", "127.0.0.1:1")),
            "Fixture context leaked into tool definitions")
        return generated


def atomic_write(path, payload):
    """Leave the original intact until the same-directory replacement succeeds."""
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix=".lhm-manifest-",
                                         suffix=".tmp", delete=False) as output:
            temporary = Path(output.name)
            output.write(payload)
            output.flush()
            os.fsync(output.fileno())
            os.fchmod(output.fileno(), stat.S_IMODE(path.stat().st_mode))
        os.replace(temporary, path)
    finally:
        if temporary is not None and temporary.exists():
            temporary.unlink()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True, help="Trusted SafeSelect executable")
    parser.add_argument("--postgres-driver", type=Path, required=True, help="Existing PostgreSQL JDBC JAR")
    parser.add_argument("--driver-sha256", required=True, help="Independently verified expected JAR checksum")
    parser.add_argument("--manifest", type=Path,
                        default=Path(__file__).resolve().parents[2] / "lhm.plugin.json")
    parser.add_argument("--check", action="store_true", help="Exit 1 on drift without writing")
    args = parser.parse_args()
    try:
        manifest = load_manifest(args.manifest.read_text(encoding="utf-8"))
        validate_manifest(manifest)
        generated = generate(args.binary.resolve(strict=True), args.postgres_driver.resolve(strict=True),
                             args.driver_sha256, manifest["version"])
        updated = {**manifest, **generated}
        validate_manifest(updated)
        if all(manifest.get(field) == generated[field] for field in generated):
            print("Manifest is current: " + ", ".join(
                f"{len(generated[field])} {field}" for field in ("tools", "prompts", "resources")))
            return 0
        if args.check:
            print("Manifest tools differ; rerun without --check to update")
            return 1
        atomic_write(args.manifest,
            (json.dumps(updated, indent=2, ensure_ascii=False, allow_nan=False) + "\n").encode("utf-8"))
        print("Updated " + ", ".join(
            f"{len(generated[field])} {field}" for field in ("tools", "prompts", "resources"))
            + "; other metadata unchanged")
        return 0
    except ManifestError as error:
        print(f"Manifest validation failed: {error}")
        return 2
    except (OSError, ValueError, KeyError, TypeError, subprocess.TimeoutExpired):
        # Do not print subprocess output or potentially private local configuration.
        print("Introspection failed. Check binary/version, driver checksum and MCP response compatibility.")
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
