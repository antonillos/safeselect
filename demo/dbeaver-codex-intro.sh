#!/usr/bin/env bash
set -euo pipefail

: "${SAFESELECT_DBEAVER_ROOT:?source demo.env first}"
DBP="${SAFESELECT_DBEAVER_ROOT}/dbeaver-demo.dbp"

python3 - "${DBP}" <<'PY'
import json
import sys
import zipfile

with zipfile.ZipFile(sys.argv[1]) as archive:
    payload = json.loads(
        archive.read("projects/staging/.dbeaver/data-sources.json")
    )

connection = next(iter(payload["connections"].values()))
configuration = connection["configuration"]
tunnel = configuration["handlers"]["ssh_tunnel"]["properties"]
print("\n=== DBeaver export → Codex handoff ===")
print("Archive: dbeaver-demo.dbp (deterministic DBeaver export)")
print("Project: staging / PostgreSQL Demo")
print(f"Driver: {connection['driver']}")
print(f"JDBC URL: {configuration['url']}")
print(
    f"SSH tunnel: {tunnel['userName']}@{tunnel['host']}:{tunnel['port']}"
    " → postgres:5432"
)
print("Database auth: demo + Keychain password (not in export)")
print("SSH auth: ephemeral Ed25519 key file (not in export)")
print("Import state: imported connection ready for Codex")
PY
