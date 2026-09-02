#!/usr/bin/env bash
set -euo pipefail

: "${SAFESELECT_DBEAVER_ROOT:?source demo.env first}"
: "${SAFESELECT_DBEAVER_PROJECT:?source demo.env first}"
: "${CODEX_HOME:?source demo.env first}"
PROJECT_FILE="${SAFESELECT_DBEAVER_PROJECT}/.safeselect/project.toml"
AUDIT_DIR="${SAFESELECT_DBEAVER_ROOT}/audit"
[[ -f "${PROJECT_FILE}" ]] || {
  echo "Imported SafeSelect project not found: ${PROJECT_FILE}" >&2
  echo "Run the existing interactive DBeaver import tape first, then rerun this Codex tape." >&2
  exit 1
}
mkdir -p "${AUDIT_DIR}"
# The runtime gets its own Codex home.  Start from an empty global config so
# connectors configured in the user's normal Codex profile cannot leak into
# this recording; the project-scoped SafeSelect entry is installed afterward.
mkdir -p "${CODEX_HOME}"
cat > "${CODEX_HOME}/config.toml" <<EOF
# Deliberately no global MCP servers: the demo allowlist is project-scoped.
[projects."${SAFESELECT_DBEAVER_PROJECT}"]
trust_level = "trusted"
EOF
python3 - "${PROJECT_FILE}" "${AUDIT_DIR}" <<'PY'
from pathlib import Path
import sys
project = Path(sys.argv[1])
audit = Path(sys.argv[2])
text = project.read_text()
old = 'directory = "~/.local/state/safeselect/audit"'
new = f'directory = "{audit}"'
if old in text:
    text = text.replace(old, new, 1)
elif f'directory = "{audit}"' not in text:
    raise SystemExit("expected audit directory setting was not found")
project.write_text(text)
PY
printf 'Isolated audit directory: %s\n' "${AUDIT_DIR}"
printf 'Trusted Codex project: %s\n' "${SAFESELECT_DBEAVER_PROJECT}"
