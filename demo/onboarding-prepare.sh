#!/usr/bin/env bash
set -euo pipefail

SOURCE="${BASH_SOURCE[0]}"
while [ -h "${SOURCE}" ]; do
  SOURCE_DIR="$(CDPATH= cd -- "$(dirname "${SOURCE}")" && pwd)"
  LINK_TARGET="$(readlink "${SOURCE}")"
  case "${LINK_TARGET}" in
    /*) SOURCE="${LINK_TARGET}" ;;
    *) SOURCE="${SOURCE_DIR}/${LINK_TARGET}" ;;
  esac
done
ROOT_DIR="$(CDPATH= cd -- "$(dirname "${SOURCE}")" && pwd -P)"
source "${ROOT_DIR}/env.sh"

SSH_DIR="${ROOT_DIR}/.runtime/ssh"
SSH_KEY="${SSH_DIR}/demo_ed25519"
ONBOARDING_DIR="/private/tmp/safeselect-onboarding-full-local"

mkdir -p "${ONBOARDING_DIR}"
if command -v safeselect >/dev/null 2>&1; then
  (
    cd "${ONBOARDING_DIR}"
    safeselect agent uninstall opencode \
      --name safeselect-safeselect-onboarding-full-local-staging \
      >/dev/null 2>&1 || true
  )
fi
# Keep the recording deterministic without touching the user's real SafeSelect
# configuration. The onboarding project and its Keychain account are demo-only.
rm -rf "${ONBOARDING_DIR}/.safeselect"
security delete-generic-password \
  -s safeselect \
  -a safeselect-onboarding-full-local/staging \
  >/dev/null 2>&1 || true

mkdir -p "${SSH_DIR}"
if [ ! -f "${SSH_KEY}" ]; then
  ssh-keygen -q -t ed25519 -N "" -C "safeselect-demo" -f "${SSH_KEY}"
fi
cp "${SSH_KEY}.pub" "${SSH_DIR}/authorized_keys"
chmod 600 "${SSH_KEY}" "${SSH_DIR}/authorized_keys"

"${ROOT_DIR}/demo.sh" start

printf '\nOnboarding SSH fixture is ready.\n'
printf '  Bastion: 127.0.0.1:55222 (demo user)\n'
printf '  Key:     %s\n' "${SSH_KEY}"
printf '  Target:  postgres:5432 (inside the demo network)\n'
