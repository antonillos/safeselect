#!/usr/bin/env python3
"""Validate the shared SafeSelect CLI gallery and its committed captures."""

from __future__ import annotations

import binascii
import json
import struct
import zlib
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


def valid_png(path: Path) -> bool:
    data = path.read_bytes()
    signature = b"\x89PNG\r\n\x1a\n"
    if not data.startswith(signature):
        return False
    offset = len(signature)
    saw_ihdr = False
    saw_idat = False
    saw_iend = False
    idat_chunks: list[bytes] = []
    width = height = bit_depth = color_type = interlace = 0
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}
    while offset + 12 <= len(data):
        length = struct.unpack(">I", data[offset : offset + 4])[0]
        chunk_type = data[offset + 4 : offset + 8]
        end = offset + 12 + length
        if end > len(data):
            return False
        chunk_data = data[offset + 8 : offset + 8 + length]
        stored_crc = struct.unpack(">I", data[offset + 8 + length : end])[0]
        if binascii.crc32(chunk_type + chunk_data) & 0xFFFFFFFF != stored_crc:
            return False
        if chunk_type == b"IHDR":
            if saw_ihdr or offset != len(signature) or length != 13:
                return False
            width, height, bit_depth, color_type, compression, filtering, interlace = struct.unpack(
                ">IIBBBBB", chunk_data
            )
            if (
                width == 0
                or height == 0
                or compression != 0
                or filtering != 0
                or interlace != 0
                or color_type not in channels
                or bit_depth != 8
            ):
                return False
            saw_ihdr = True
        elif chunk_type == b"IDAT":
            if not saw_ihdr or saw_iend:
                return False
            saw_idat = True
            idat_chunks.append(chunk_data)
        elif chunk_type == b"IEND":
            if length != 0 or not saw_ihdr or not saw_idat:
                return False
            saw_iend = True
            if end != len(data):
                return False
            try:
                decoder = zlib.decompressobj()
                pixels = decoder.decompress(b"".join(idat_chunks), 64 * 1024 * 1024 + 1)
                if decoder.unconsumed_tail:
                    return False
                pixels += decoder.flush()
            except zlib.error:
                return False
            if decoder.unused_data or not decoder.eof:
                return False
            row_bytes = width * channels[color_type]
            expected = height * (row_bytes + 1)
            if expected > 64 * 1024 * 1024 or len(pixels) != expected:
                return False
            return all(pixels[row * (row_bytes + 1)] <= 4 for row in range(height))
        offset = end
    return False


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

    seen_images: set[str] = set()
    for item in commands:
        image_name = Path(item["image"]).name
        expected_image = f"{item['id']}.png"
        if image_name != expected_image:
            raise SystemExit(
                f"capture mapping mismatch for {item['id']}: expected {expected_image}, got {item['image']}"
            )
        if item["image"] in seen_images:
            raise SystemExit(f"duplicate capture mapping: {item['image']}")
        seen_images.add(item["image"])
        image = ROOT / "docs" / "recordings" / item["image"]
        if not image.is_file():
            raise SystemExit(f"missing capture for {item['id']}: {image}")
        if not valid_png(image):
            raise SystemExit(f"capture is not a valid PNG: {image}")
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
