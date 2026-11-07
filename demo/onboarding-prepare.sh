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

if [[ -L "${ONBOARDING_DIR}" ]]; then
  printf '%s\n' 'Refusing a symlink onboarding directory.' >&2
  exit 1
fi
# Reset only the disposable recording project and its isolated configuration.
rm -rf "${ONBOARDING_DIR}"
mkdir -p "${ONBOARDING_DIR}"/{config/homebrew,cache,runtime}
# Preserve existing Homebrew trust choices in the isolated recording profile.
if [[ -f "${HOME}/.homebrew/trust.json" ]]; then
  cp "${HOME}/.homebrew/trust.json" "${ONBOARDING_DIR}/config/homebrew/trust.json"
fi
printf '%s\n' '.safeselect/' '.opencode/' > "${ONBOARDING_DIR}/.gitignore"
python3 "${ROOT_DIR}/fixtures/dbeaver/build_fixture.py" \
  --output "${ONBOARDING_DIR}/dbeaver-demo.dbp"
python3 - "${ONBOARDING_DIR}/opencode.json" <<'PYTHON'
import json
import sys
from pathlib import Path

global_path = Path.home() / ".config/opencode/opencode.json"
global_config = json.loads(global_path.read_text()) if global_path.exists() else {}
servers = global_config.get("mcp", {})
servers = servers.get("servers", servers)
config = {
    "$schema": "https://opencode.ai/config.json",
    "permission": {"*": "deny", "execute": "allow", "safeselect*": "allow"},
    "provider": {"openai": {"models": {"gpt-6-luna": {
        "options": {"reasoningSummary": "detailed"}
    }}}},
    "mcp": {"servers": {
        name: {"type": "local", "command": ["false"], "disabled": True}
        for name in servers
    }},
}
Path(sys.argv[1]).write_text(json.dumps(config, indent=2) + "\n")
PYTHON
cp "${ROOT_DIR}/onboarding-colors.py" "${ONBOARDING_DIR}/onboarding-colors.py"
cat > "${ONBOARDING_DIR}/recording.env" <<EOF
export SAFESELECT_CONFIG_DIR='${ONBOARDING_DIR}/runtime'
export XDG_CONFIG_HOME='${ONBOARDING_DIR}/config'
export XDG_CACHE_HOME='${ONBOARDING_DIR}/cache'
export PATH='${ONBOARDING_DIR}/bin':"\$PATH"
export HOMEBREW_NO_AUTO_UPDATE=1
export HOMEBREW_NO_ENV_HINTS=1
export TERM=xterm-256color COLORTERM=truecolor CLICOLOR=1 CLICOLOR_FORCE=1 FORCE_COLOR=3
export PS1='\[\e[36m\]demo \[\e[32m\]$ \[\e[0m\]'
unset NO_COLOR SAFESELECT_DEMO_PASSWORD
opencode() {
  if [[ "\${1:-}" == run ]]; then
    command opencode "\$@" 2>&1 | python3 '${ONBOARDING_DIR}/onboarding-colors.py' '${ONBOARDING_DIR}/agent-original.txt'
    return \${PIPESTATUS[0]}
  fi
  command opencode "\$@"
}
EOF

mkdir -p "${SSH_DIR}"
if [ ! -f "${SSH_KEY}" ]; then
  ssh-keygen -q -t ed25519 -N "" -C "safeselect-demo" -f "${SSH_KEY}"
fi
cp "${SSH_KEY}.pub" "${SSH_DIR}/authorized_keys"
chmod 600 "${SSH_KEY}"
chmod 644 "${SSH_DIR}/authorized_keys"
mkdir -p "${ONBOARDING_DIR}/ssh"
cp "${SSH_KEY}" "${ONBOARDING_DIR}/ssh/demo_ed25519"

"${ROOT_DIR}/demo.sh" start
# Pin the disposable bastion's public host key without changing ~/.ssh.
docker compose -p safeselect-demo -f "${ROOT_DIR}/docker-compose.yml" \
  exec -T ssh-bastion cat /etc/ssh/ssh_host_ed25519_key.pub \
  | awk '{print "[127.0.0.1]:55222 " $1 " " $2}' > "${ONBOARDING_DIR}/ssh/known_hosts"
cat > "${ONBOARDING_DIR}/pin-bastion.py" <<'PYTHON'
from pathlib import Path

root = Path(__file__).resolve().parent
path = root / ".safeselect/project.toml"
text = path.read_text()
section = "[ssh_bastions.demo-55222]\n"
assert text.count(section) == 1
text = text.replace(section, section + f'known_hosts = "{root}/ssh/known_hosts"\n')
text = text.replace('directory = "~/.local/state/safeselect/audit"',
                    f'directory = "{root}/audit"')
path.write_text(text)
PYTHON

printf '\nOnboarding SSH fixture is ready.\n'
printf '  Bastion: 127.0.0.1:55222 (demo user)\n'
printf '  Key:     %s\n' "${ONBOARDING_DIR}/ssh/demo_ed25519"
printf '  Target:  postgres:5432 (inside the demo network)\n'
