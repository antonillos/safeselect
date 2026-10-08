#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)
runtime=/tmp/safeselect-dbeaver-ux
if [[ ! -x "$repo/target/debug/safeselect" ]]; then
  printf '%s\n' 'Build the current Rust CLI with cargo build first.' >&2
  exit 1
fi
# Reset only this disposable runtime; never load real DBeaver exports.
if [[ -L "$runtime" ]]; then
  printf '%s\n' 'Refusing a symlink runtime directory.' >&2
  exit 1
fi
rm -rf -- "$runtime"
mkdir -p "$runtime/atlas" "$runtime/bin" "$runtime/global"
printf '%s\n' '.safeselect/' > "$runtime/atlas/.gitignore"
cp "$repo/target/debug/safeselect" "$runtime/bin/safeselect"
python3 - "$repo/demo/dbeaver-import-ux/data-sources.json" "$runtime/dbeaver.zip" <<'PY'
import sys
import zipfile
with zipfile.ZipFile(sys.argv[2], 'w') as archive:
    archive.write(sys.argv[1], 'workspace/data-sources.json')
PY
printf '%s\n' 'Prepared a synthetic offline DBeaver import. No Keychain writes or database checks.'
