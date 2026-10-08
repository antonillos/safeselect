#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)
runtime=/tmp/safeselect-compass-reimport
if [[ ! -x "$repo/target/debug/safeselect" ]]; then
  printf '%s\n' 'Build the current Rust CLI with cargo build first.' >&2
  exit 1
fi
if [[ -L "$runtime" ]]; then
  printf '%s\n' 'Refusing a symlink runtime directory.' >&2
  exit 1
fi
rm -rf -- "$runtime"
mkdir -p "$runtime/atlas" "$runtime/bin" "$runtime/global"
cp "$repo/target/debug/safeselect" "$runtime/bin/safeselect"
cp "$repo/demo/compass-import-ux/compass.json" "$runtime/compass.json"
# Seed a password-free imported environment; no connections or Keychain writes.
(cd "$runtime/atlas" && SAFESELECT_CONFIG_DIR="$runtime/global" \
  "$runtime/bin/safeselect" import-compass --non-interactive --path ../compass.json >/dev/null)
printf '%s\n' 'Prepared a separate synthetic reimport demo with one existing environment.'
