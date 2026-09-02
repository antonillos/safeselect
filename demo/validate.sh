#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(CDPATH= cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${ROOT_DIR}/.." && pwd)"

bash -n "${ROOT_DIR}"/*.sh
CODEX_INTEGRATION_ROOT="${TMPDIR:-/tmp}/safeselect-codex-agent" \
  bash -n "${ROOT_DIR}/codex-setup.sh"
for tape in "${ROOT_DIR}"/safeselect-*.tape; do
  vhs validate "${tape}"
done

# shellcheck source=env.sh
source "${ROOT_DIR}/env.sh"
safeselect config validate --project "${ROOT_DIR}" --environment postgres
safeselect config validate --project "${ROOT_DIR}" --environment mongodb

fixture_tmp="$(mktemp -d)"
trap 'rm -rf "${fixture_tmp}"' EXIT
python3 "${ROOT_DIR}/fixtures/dbeaver/build_fixture.py" --output "${fixture_tmp}/one.dbp"
python3 "${ROOT_DIR}/fixtures/dbeaver/build_fixture.py" --output "${fixture_tmp}/two.dbp"
cmp "${fixture_tmp}/one.dbp" "${fixture_tmp}/two.dbp"
if rg -n --hidden --glob '!*.pyc' '/Users/|antonillos|password|credential' \
  "${ROOT_DIR}/fixtures/dbeaver"; then
  printf '%s\n' 'DBeaver fixture contains a private path or credential marker.' >&2
  exit 1
fi

printf '%s\n' 'Demo configuration, scripts, and VHS tape are valid.'
