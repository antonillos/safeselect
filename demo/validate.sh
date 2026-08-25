#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(CDPATH= cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${ROOT_DIR}/.." && pwd)"

bash -n "${ROOT_DIR}"/*.sh
vhs validate "${ROOT_DIR}/safeselect-demo.tape"

# shellcheck source=env.sh
source "${ROOT_DIR}/env.sh"
safeselect config validate --project "${ROOT_DIR}" --environment postgres
safeselect config validate --project "${ROOT_DIR}" --environment mongodb

printf '%s\n' 'Demo configuration, scripts, and VHS tape are valid.'
