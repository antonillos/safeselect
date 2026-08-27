#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(CDPATH= cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${ROOT_DIR}/env.sh"

SSH_DIR="${ROOT_DIR}/.runtime/ssh"
SSH_KEY="${SSH_DIR}/demo_ed25519"
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
