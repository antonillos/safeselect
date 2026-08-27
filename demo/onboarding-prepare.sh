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
# Remove only demo-scoped state before recording. The install itself remains
# visible in the tape and always comes from the released Homebrew formula.
if command -v safeselect >/dev/null 2>&1; then
  (
    cd "${ONBOARDING_DIR}"
    safeselect agent uninstall opencode \
      --name safeselect-safeselect-onboarding-full-local-staging \
      >/dev/null 2>&1 || true
  )
fi
if command -v brew >/dev/null 2>&1; then
  brew uninstall --formula safeselect >/dev/null 2>&1 || true
  brew untap --force antonillos/tap >/dev/null 2>&1 || true
fi
# Keep the recording deterministic without touching the user's real SafeSelect
# configuration. The onboarding project and its Keychain account are demo-only.
rm -rf "${ONBOARDING_DIR}/.homebrew" \
       "${ONBOARDING_DIR}/.opencode" \
       "${ONBOARDING_DIR}/.safeselect" \
       "${ONBOARDING_DIR}/bin"

# Isolate Homebrew trust metadata as well. Preserve the user’s unrelated trust
# choices but make this formula untrusted so the tape shows the exact approval.
DEMO_HOMEBREW_CONFIG="${ONBOARDING_DIR}/.homebrew/homebrew"
mkdir -p "${DEMO_HOMEBREW_CONFIG}"
if [ -f "${HOME}/.homebrew/trust.json" ]; then
  python3 - "${HOME}/.homebrew/trust.json" "${DEMO_HOMEBREW_CONFIG}/trust.json" <<'PYTHON'
import json
import sys
from pathlib import Path

source, destination = map(Path, sys.argv[1:])
data = json.loads(source.read_text())
for key in ("trustedtaps", "trustedformulae", "taps", "formulae"):
    data[key] = [item for item in data.get(key, []) if not item.startswith("antonillos/tap")]
destination.write_text(json.dumps(data, indent=2) + "\n")
PYTHON
fi

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
