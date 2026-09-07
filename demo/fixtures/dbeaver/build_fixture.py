#!/usr/bin/env python3
"""Build the public, deterministic DBeaver fixture used by the SSH demo."""

from __future__ import annotations

import argparse
import json
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parent
DATA_SOURCES = ROOT / "data-sources.json"


def build(destination: Path) -> None:
    payload = json.loads(DATA_SOURCES.read_text(encoding="utf-8"))
    content = json.dumps(payload, indent=2, sort_keys=False) + "\n"
    destination.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(
        destination, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9
    ) as archive:
        for name, data in (
            ("projects/staging/.dbeaver/data-sources.json", content.encode("utf-8")),
            ("meta.xml", b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"),
        ):
            info = zipfile.ZipInfo(name, date_time=(2020, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            archive.writestr(info, data)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    build(args.output)


if __name__ == "__main__":
    main()
