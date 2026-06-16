#!/usr/bin/env bash
# uninstall.sh — remove safeselect and its configuration
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="${BIN_DIR:-${PREFIX}/bin}"

confirm() {
  printf '%s [y/N] ' "$1"
  read -r reply
  case "$reply" in
    [yY]|[yY][eE][sS]) return 0 ;;
    *) return 1 ;;
  esac
}

printf 'SafeSelect Uninstall\n'
printf '===================\n\n'

# 1. Remove binary
if [ -f "${BIN_DIR}/safeselect" ]; then
  printf '  Binary: %s/safeselect\n' "${BIN_DIR}"
else
  printf '  Binary: not found at %s/safeselect\n' "${BIN_DIR}"
fi

# 2. Detect config dir
CONFIG_DIR=""
if [ -d "${HOME}/Library/Application Support/safeselect" ]; then
  CONFIG_DIR="${HOME}/Library/Application Support/safeselect"
elif [ -d "${HOME}/.config/safeselect" ]; then
  CONFIG_DIR="${HOME}/.config/safeselect"
fi

if [ -n "${CONFIG_DIR}" ]; then
  printf '  Config: %s\n' "${CONFIG_DIR}"
else
  printf '  Config: not found\n'
fi

# 3. Detect data dir (sidecar JAR)
DATA_DIR="${HOME}/.local/share/safeselect"
if [ -d "${DATA_DIR}" ]; then
  printf '  Data:   %s\n' "${DATA_DIR}"
fi

# 4. Detect audit logs
AUDIT_DIR="${HOME}/.local/state/safeselect"
if [ -d "${AUDIT_DIR}" ]; then
  printf '  Audit:  %s\n' "${AUDIT_DIR}"
fi

echo

if ! confirm "Remove all safeselect files?"; then
  printf 'Cancelled.\n'
  exit 0
fi

# Remove binary
rm -f "${BIN_DIR}/safeselect"
printf '  ✓ Removed %s/safeselect\n' "${BIN_DIR}"

# Remove config
if [ -n "${CONFIG_DIR}" ]; then
  rm -rf "${CONFIG_DIR}"
  printf '  ✓ Removed %s\n' "${CONFIG_DIR}"
fi

# Remove data
rm -rf "${DATA_DIR}"
printf '  ✓ Removed %s\n' "${DATA_DIR}"

# Remove audit
rm -rf "${AUDIT_DIR}"
printf '  ✓ Removed %s\n' "${AUDIT_DIR}"

# Remove any safeselect backups in agent configs
for f in "${HOME}/Library/Application Support/opencode/opencode.json.safeselect.bak" \
         "${HOME}/.config/opencode/opencode.json.safeselect.bak" \
         "${HOME}/.cursor/config.json.safeselect.bak" \
         "${HOME}/.windsurf/config.json.safeselect.bak"; do
  [ -f "$f" ] && rm -f "$f" && printf '  ✓ Removed backup %s\n' "$f"
done

echo
printf 'Uninstall complete.\n'

# List remaining safeselect entries in agent configs
MCP_FILES=()
[ -f "${HOME}/Library/Application Support/opencode/opencode.json" ] && MCP_FILES+=("${HOME}/Library/Application Support/opencode/opencode.json")
[ -f "${HOME}/.config/opencode/opencode.json" ] && MCP_FILES+=("${HOME}/.config/opencode/opencode.json")
[ -f "${HOME}/.cursor/config.json" ] && MCP_FILES+=("${HOME}/.cursor/config.json")
[ -f "${HOME}/.windsurf/config.json" ] && MCP_FILES+=("${HOME}/.windsurf/config.json")

for f in "${MCP_FILES[@]}"; do
  if grep -q "safeselect" "$f" 2>/dev/null; then
    printf '\n⚠  Remove safeselect entries from %s manually.\n' "$f"
  fi
done

# Check for macOS Keychain entries
if command -v security &>/dev/null; then
  if security find-generic-password -s "safeselect" 2>/dev/null | grep -q "safeselect"; then
    printf '\n⚠  macOS Keychain entries for "safeselect" remain.\n'
    printf '   Remove with: security delete-generic-password -s safeselect\n'
  fi
fi
