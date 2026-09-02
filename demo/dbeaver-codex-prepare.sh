#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(CDPATH= cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUN_ROOT="${SAFESELECT_DBEAVER_ROOT:-/private/tmp/safeselect-dbeaver-codex}"
HOST_RUNTIME_ROOT="${ROOT_DIR}/.demo-dbeaver-runtime"
HOST_SSH_DIR="${HOST_RUNTIME_ROOT}/ssh"
RELEASE_VERSION="0.7.7"
RELEASE_BASE="https://github.com/antonillos/safeselect/releases/download/v${RELEASE_VERSION}"

case "$(uname -s)" in
  Darwin) ;;
  *) echo "This DBeaver SSH demo currently supports macOS only (Keychain credentials required)." >&2; exit 1 ;;
esac

# This script resets the entire runtime. Keep overrides confined to an
# explicitly disposable /private/tmp directory before deleting anything.
RUN_ROOT="$(python3 - "${RUN_ROOT}" <<'PY'
from pathlib import Path
import sys

print(Path(sys.argv[1]).resolve(strict=False))
PY
)"
case "${RUN_ROOT}" in
  /private/tmp/safeselect-dbeaver-?*) ;;
  *)
    echo "SAFESELECT_DBEAVER_ROOT must be a disposable /private/tmp/safeselect-dbeaver-* directory: ${RUN_ROOT}" >&2
    exit 1
    ;;
esac

rm -rf "${RUN_ROOT}"
mkdir -p "${RUN_ROOT}"/bin "${RUN_ROOT}"/codex-home "${RUN_ROOT}"/safeselect-dbeaver-demo \
  "${RUN_ROOT}"/runtime "${RUN_ROOT}"/ssh
rm -rf "${HOST_RUNTIME_ROOT}"
mkdir -p "${HOST_SSH_DIR}"

SAFESELECT_BIN_PATH=""
if [[ -n "${SAFESELECT_BIN:-}" ]]; then
  SAFESELECT_BIN_PATH="$(CDPATH= cd -- "$(dirname "${SAFESELECT_BIN}")" 2>/dev/null && pwd)/$(basename "${SAFESELECT_BIN}")" || true
  if [[ ! -x "${SAFESELECT_BIN_PATH}" ]]; then
    echo "Ignoring missing SAFESELECT_BIN override; downloading public SafeSelect ${RELEASE_VERSION}." >&2
    SAFESELECT_BIN_PATH=""
  fi
fi

if [[ -z "${SAFESELECT_BIN_PATH:-}" ]]; then
  case "$(uname -s):$(uname -m)" in
    Darwin:arm64) PLATFORM="aarch64-apple-darwin" ;;
    Darwin:x86_64) PLATFORM="x86_64-apple-darwin" ;;
    *) echo "Unsupported platform: $(uname -s):$(uname -m)" >&2; exit 1 ;;
  esac
  ARCHIVE="safeselect-v${RELEASE_VERSION}-${PLATFORM}.tar.gz"
  curl --fail --location --silent --show-error \
    "${RELEASE_BASE}/${ARCHIVE}" -o "${RUN_ROOT}/${ARCHIVE}"
  curl --fail --location --silent --show-error \
    "${RELEASE_BASE}/${ARCHIVE}.sha256" -o "${RUN_ROOT}/${ARCHIVE}.sha256"
  (cd "${RUN_ROOT}" && shasum -a 256 -c "${ARCHIVE}.sha256")
  tar -xzf "${RUN_ROOT}/${ARCHIVE}" -C "${RUN_ROOT}/bin"
  SAFESELECT_BIN_PATH="$(find "${RUN_ROOT}/bin" -type f -name safeselect -perm -111 -print -quit)"
fi

SAFESELECT_BIN_DIR="$(dirname "${SAFESELECT_BIN_PATH}")"

if [[ -z "${SAFESELECT_BIN_PATH}" || ! -x "${SAFESELECT_BIN_PATH}" ]]; then
  echo "SafeSelect binary was not found" >&2
  exit 1
fi
VERSION_OUTPUT="$(${SAFESELECT_BIN_PATH} --version)"
case "${VERSION_OUTPUT}" in
  "safeselect v${RELEASE_VERSION}"*|"safeselect ${RELEASE_VERSION}"*) ;;
  *) echo "Expected SafeSelect ${RELEASE_VERSION}, got: ${VERSION_OUTPUT}" >&2; exit 1 ;;
esac

SSH_KEY="${RUN_ROOT}/ssh/demo_ed25519"
ssh-keygen -q -t ed25519 -N "" -C "safeselect-dbeaver-demo" -f "${SSH_KEY}"
chmod 600 "${SSH_KEY}"
cp "${SSH_KEY}.pub" "${RUN_ROOT}/ssh/authorized_keys"
chmod 644 "${RUN_ROOT}/ssh/authorized_keys"
cp "${SSH_KEY}.pub" "${HOST_SSH_DIR}/authorized_keys"
chmod 644 "${HOST_SSH_DIR}/authorized_keys"

python3 "${ROOT_DIR}/fixtures/dbeaver/build_fixture.py" \
  --output "${RUN_ROOT}/dbeaver-demo.dbp"
python3 "${ROOT_DIR}/fixtures/dbeaver/build_fixture.py" \
  --output "${RUN_ROOT}/dbeaver-demo-repeat.dbp"
cmp "${RUN_ROOT}/dbeaver-demo.dbp" "${RUN_ROOT}/dbeaver-demo-repeat.dbp"
rm "${RUN_ROOT}/dbeaver-demo-repeat.dbp"
cat > "${HOST_RUNTIME_ROOT}/postgres-readonly.sql" <<'SQL'

-- The DBeaver→Codex fixture keeps the documented demo/demo-password login as
-- a separate role. Even if an agent obtains the macOS Keychain item,
-- PostgreSQL itself rejects writes outside SafeSelect.
CREATE ROLE demo LOGIN PASSWORD 'demo-password' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA public TO demo;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO demo;
ALTER DEFAULT PRIVILEGES FOR ROLE safeselect_dbeaver_admin IN SCHEMA public
  GRANT SELECT ON TABLES TO demo;
SQL
cp "${ROOT_DIR}/dbeaver-codex-run.sh" "${RUN_ROOT}/dbeaver-codex-run.sh"
chmod 700 "${RUN_ROOT}/dbeaver-codex-run.sh"
cp "${ROOT_DIR}/dbeaver-codex-isolate.sh" "${RUN_ROOT}/dbeaver-codex-isolate.sh"
chmod 755 "${RUN_ROOT}/dbeaver-codex-isolate.sh"
cp "${ROOT_DIR}/dbeaver-codex-format.py" "${RUN_ROOT}/dbeaver-codex-format.py"
chmod 755 "${RUN_ROOT}/dbeaver-codex-format.py"
cp "${ROOT_DIR}/dbeaver-codex-intro.sh" "${RUN_ROOT}/dbeaver-codex-intro.sh"
chmod 755 "${RUN_ROOT}/dbeaver-codex-intro.sh"

cat > "${RUN_ROOT}/compose.override.yml" <<EOF
services:
  postgres:
    environment:
      POSTGRES_USER: safeselect_dbeaver_admin
      POSTGRES_PASSWORD: safeselect-dbeaver-admin
    volumes:
      - ${HOST_RUNTIME_ROOT}/postgres-readonly.sql:/docker-entrypoint-initdb.d/02-dbeaver-readonly.sql:ro
  ssh-bastion:
    volumes:
      - ${HOST_SSH_DIR}/authorized_keys:/home/demo/.ssh/authorized_keys:ro
EOF

if ! docker info >/dev/null 2>&1; then
  if command -v colima >/dev/null 2>&1; then
    colima start
  fi
fi
docker info >/dev/null

STANDARD_COMPOSE=(docker compose -p safeselect-demo -f "${ROOT_DIR}/docker-compose.yml")
if [[ -n "$("${STANDARD_COMPOSE[@]}" ps -q 2>/dev/null)" ]]; then
  printf '%s\n' 'Stopping the standard demo fixture to release its documented ports (volumes are preserved).'
  "${STANDARD_COMPOSE[@]}" down --remove-orphans >/dev/null
fi

COMPOSE=(docker compose -p safeselect-dbeaver-codex \
  -f "${ROOT_DIR}/docker-compose.yml" -f "${RUN_ROOT}/compose.override.yml")
"${COMPOSE[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
"${COMPOSE[@]}" up -d --wait

cat > "${RUN_ROOT}/demo.env" <<EOF
export SAFESELECT_BIN='${SAFESELECT_BIN_PATH}'
export SAFESELECT_CONFIG_DIR='${RUN_ROOT}/runtime'
unset SAFESELECT_DEMO_PASSWORD
export SAFESELECT_DBEAVER_ROOT='${RUN_ROOT}'
export SAFESELECT_DBEAVER_SOURCE='${ROOT_DIR}'
export CODEX_HOME='${RUN_ROOT}/codex-home'
export SAFESELECT_DBEAVER_COMPOSE='${RUN_ROOT}/compose.override.yml'
export SAFESELECT_DBEAVER_PROJECT='${RUN_ROOT}/safeselect-dbeaver-demo'
export PATH='${SAFESELECT_BIN_DIR}':"\$PATH"
EOF

printf '%s\n' "Prepared isolated DBeaver → Codex demo at ${RUN_ROOT}"
printf '%s\n' "Next: run the interactive DBeaver import with ${RUN_ROOT}/demo.env."
printf '%s\n' "After import, source ${RUN_ROOT}/demo.env; if needed, run CODEX_HOME=\"\$CODEX_HOME\" codex login, then run demo/dbeaver-codex.tape."
