#!/usr/bin/env python3
"""Validate the shared SafeSelect CLI gallery and its committed captures."""

from __future__ import annotations

import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CATALOG = ROOT / "docs" / "cli-gallery.json"
README = ROOT / "README.md"
EXPECTED = {
    "list_tables",
    "describe_table",
    "get_maintenance_diagnostics",
    "list_databases",
    "list_collections",
    "discover_document_schema",
    "find_documents",
    "serve",
    "config",
    "driver",
    "agent",
    "import-dbeaver",
    "import-compose",
    "import-compass",
    "check",
    "doctor",
    "posture",
    "query",
    "disconnect",
    "connect",
    "reconnect",
}


def main() -> int:
    catalog = json.loads(CATALOG.read_text(encoding="utf-8"))
    groups = catalog.get("groups", [])
    commands = [item for group in groups for item in group.get("commands", [])]
    ids = [item["id"] for item in commands]
    if set(ids) != EXPECTED or len(ids) != len(EXPECTED):
        raise SystemExit(f"CLI gallery coverage mismatch: {sorted(ids)}")
    if [group["id"] for group in groups][-2:] != ["query", "nosql"]:
        raise SystemExit("query and nosql must be the final separate workflows")
    if {item["id"] for item in groups[-2]["commands"]} != {"query", "list_tables", "describe_table", "get_maintenance_diagnostics"}:
        raise SystemExit("query must remain the SQL workflow with its NoSQL sibling")
    readme = README.read_text(encoding="utf-8")
    for item in commands:
        if f"docs/recordings/{item['image']}" not in readme:
            raise SystemExit(f"README is missing the shared capture for {item['id']}")

    for item in commands:
        image = ROOT / "docs" / "recordings" / item["image"]
        if not image.is_file():
            raise SystemExit(f"missing capture for {item['id']}: {image}")
        if image.stat().st_size > 250_000:
            raise SystemExit(f"capture is too large for the gallery: {image}")
        text = " ".join(
            str(item.get(field, "")) for field in ("command", "purpose", "example", "caption")
        )
        for marker in ("/Users/", "/home/", "antonillos", "SAFESELECT_DEMO_PASSWORD"):
            if marker.lower() in text.lower():
                raise SystemExit(f"private marker in catalog entry {item['id']}: {marker}")

    print(f"Validated {len(commands)} CLI commands and {len(commands)} PNG captures.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
