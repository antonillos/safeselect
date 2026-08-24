#!/usr/bin/env bash
# Generate registry metadata after every platform MCPB artifact has a checksum.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/generate-registry-server-json.sh --version VERSION --release-url URL --output PATH PACKAGE=SHA256 [...]

Each PACKAGE=SHA256 argument names an MCPB release asset and its 64-character
SHA-256 checksum. The generated server.json is intended for the official MCP
Registry and deliberately contains no database credentials or defaults.
EOF
}

version=""
release_url=""
output=""
packages=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) version="${2:-}"; shift 2 ;;
    --release-url) release_url="${2:-}"; shift 2 ;;
    --output) output="${2:-}"; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *=*) packages+=("$1"); shift ;;
    *) printf 'Error: unknown argument: %s\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
done

case "$version" in v*) version="${version#v}" ;; esac
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]]; then
  printf 'Error: version must be semver.\n' >&2
  exit 1
fi
if [[ -z "$release_url" || -z "$output" || ${#packages[@]} -eq 0 ]]; then
  printf 'Error: --release-url, --output, and at least one PACKAGE=SHA256 are required.\n' >&2
  exit 2
fi

python3 - "$version" "$release_url" "$output" "${packages[@]}" <<'PY'
import json
import re
import sys
from pathlib import Path

version, release_url, output, *pairs = sys.argv[1:]
if not re.fullmatch(r"https://github\.com/antonillos/safeselect/releases/download/v[0-9A-Za-z._-]+", release_url):
    raise SystemExit("Error: release URL must be the canonical SafeSelect GitHub release URL.")

packages = []
for pair in pairs:
    name, checksum = pair.split("=", 1)
    if not name.endswith(".mcpb"):
        raise SystemExit(f"Error: MCPB asset must end in .mcpb: {name}")
    if not re.fullmatch(r"[a-f0-9]{64}", checksum):
        raise SystemExit(f"Error: invalid SHA-256 for {name}")
    packages.append({
        "registryType": "mcpb",
        "identifier": f"{release_url}/{name}",
        "fileSha256": checksum,
        "transport": {"type": "stdio"},
    })

document = {
    "$schema": "https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json",
    "name": "io.github.antonillos/safeselect",
    "title": "SafeSelect MCP",
    "description": "Fail-closed, read-only PostgreSQL and MongoDB access for AI agents over MCP.",
    "version": version,
    "repository": {"url": "https://github.com/antonillos/safeselect", "source": "github"},
    "packages": packages,
}

target = Path(output)
target.parent.mkdir(parents=True, exist_ok=True)
target.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
PY

printf 'Created MCP Registry metadata: %s\n' "$output"
