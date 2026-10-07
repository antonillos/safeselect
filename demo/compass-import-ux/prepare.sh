#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)
runtime=/tmp/safeselect-compass-ux
if [[ ! -x "$repo/target/debug/safeselect" ]]; then
  printf '%s\n' 'Build the current Rust CLI with cargo build first.' >&2
  exit 1
fi
# Fixed disposable directory only; no real exports, Keychain entries or database connections.
if [[ -L "$runtime" ]]; then
  printf '%s\n' 'Refusing a symlink runtime directory.' >&2
  exit 1
fi
rm -rf -- "$runtime"
mkdir -p "$runtime/atlas" "$runtime/bin" "$runtime/global"
cp "$repo/target/debug/safeselect" "$runtime/bin/safeselect"
cp "$repo/demo/compass-import-ux/compass.json" "$runtime/compass.json"
printf '%s\n' 'Prepared a synthetic Compass UX demo. No database connections will be checked.'
