#!/usr/bin/env bash
set -euo pipefail

# Run this script through a symlink from the isolated agent project. The
# fixtures stay in this repository; the agent runtime and Codex config stay
# in the integration project.
SCRIPT_PATH="${BASH_SOURCE[0]}"
INTEGRATION_ROOT="${CODEX_INTEGRATION_ROOT:-$(CDPATH= cd -- "$(dirname "${SCRIPT_PATH}")" && pwd)}"
if command -v realpath >/dev/null 2>&1; then
  SCRIPT_PATH="$(realpath "${SCRIPT_PATH}")"
fi
SAFESELECT_ROOT="${SAFESELECT_ROOT:-$(CDPATH= cd -- "$(dirname "${SCRIPT_PATH}")/.." && pwd)}"
DEMO_ROOT="${SAFESELECT_ROOT}/demo"
RUNTIME_ROOT="${DEMO_ROOT}/.runtime"
CODEX_HOME_ROOT="${INTEGRATION_ROOT}/.codex"

export SAFESELECT_CONFIG_DIR="${RUNTIME_ROOT}"
export SAFESELECT_DEMO_PASSWORD="demo-password"

"${DEMO_ROOT}/demo.sh" start

if ! safeselect driver list 2>/dev/null | grep -q 'postgresql'; then
  safeselect driver download --vendor postgresql
fi

safeselect check --project "${DEMO_ROOT}" --environment postgres
safeselect check --project "${DEMO_ROOT}" --environment mongodb

mkdir -p "${CODEX_HOME_ROOT}"
if [ -f "${HOME}/.codex/auth.json" ]; then
  ln -sfn "${HOME}/.codex/auth.json" "${CODEX_HOME_ROOT}/auth.json"
fi

CODEX_HOME="${CODEX_HOME_ROOT}" codex mcp remove safeselect-demo-postgres >/dev/null 2>&1 || true
CODEX_HOME="${CODEX_HOME_ROOT}" codex mcp add safeselect-demo-postgres \
  --env "SAFESELECT_CONFIG_DIR=${RUNTIME_ROOT}" \
  --env "SAFESELECT_DEMO_PASSWORD=${SAFESELECT_DEMO_PASSWORD}" \
  -- safeselect serve --project "${DEMO_ROOT}" --environment postgres

cat > "${INTEGRATION_ROOT}/codex.env" <<ENV
export SAFESELECT_ROOT='${SAFESELECT_ROOT}'
export SAFESELECT_CONFIG_DIR='${RUNTIME_ROOT}'
export SAFESELECT_DEMO_PASSWORD='${SAFESELECT_DEMO_PASSWORD}'
export CODEX_HOME='${CODEX_HOME_ROOT}'
ENV

printf '\nIntegration Codex setup complete.\n'
printf '  Fixture project: %s\n' "${DEMO_ROOT}"
printf '  Runtime:         %s\n' "${RUNTIME_ROOT}"
printf '  Codex project:   %s\n' "${INTEGRATION_ROOT}"
printf '\nNext:\n  source %s/codex.env\n  cd %s\n  codex mcp list\n  codex\n' "${INTEGRATION_ROOT}" "${INTEGRATION_ROOT}"
