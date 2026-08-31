#!/usr/bin/env python3
"""Idempotent publication: an existing exact version must match before it is reused."""
import argparse
import json
import os
from pathlib import Path
import time
from urllib.error import HTTPError
from urllib.parse import quote
from urllib.request import urlopen

from release import attach_file, command


def registered(metadata):
    name, version = metadata["name"], metadata["version"]
    url = ("https://registry.modelcontextprotocol.io/v0.1/servers/"
           f"{quote(name, safe='')}/versions/{quote(version, safe='')}")
    try:
        with urlopen(url, timeout=30) as response:
            actual = json.load(response)["server"]
    except HTTPError as error:
        error.close()
        if error.code == 404:
            return False
        raise
    if any(actual.get(key) != value for key, value in metadata.items()):
        raise ValueError("Registry version exists with different metadata; refusing duplicate publication")
    return True


def publish(path, publisher, repo, tag):
    metadata = json.loads(path.read_text())
    if not registered(metadata):
        command(str(publisher), "login", "github-oidc")
        command(str(publisher), "publish", str(path))
    for attempt in range(6):
        if registered(metadata):
            attach_file(repo, tag, path)
            return
        if attempt < 5:
            time.sleep(2 ** attempt)
    raise ValueError("Published version did not become visible in the registry")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--metadata", type=Path, default=Path("dist/server.json"))
    parser.add_argument("--publisher", type=Path, default=Path("mcp-publisher").resolve())
    args = parser.parse_args()
    publish(args.metadata, args.publisher, os.environ["GITHUB_REPOSITORY"], os.environ["VERSION"])
