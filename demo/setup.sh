#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(CDPATH= cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=env.sh
source "${ROOT_DIR}/env.sh"

"${ROOT_DIR}/demo.sh" start

if ! safeselect driver list 2>/dev/null | grep -q 'postgresql'; then
  safeselect driver download --vendor postgresql
fi

safeselect check --project "${ROOT_DIR}" --environment postgres
safeselect check --project "${ROOT_DIR}" --environment mongodb
