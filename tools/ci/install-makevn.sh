#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Select the host, never the Rust cross-compilation target.
case "$(uname -s):$(uname -m)" in
  Darwin:arm64|Darwin:aarch64) target=aarch64-apple-darwin ;;
  Darwin:x86_64) target=x86_64-apple-darwin ;;
  Linux:x86_64) target=x86_64-unknown-linux-gnu ;;
  *) printf 'Unsupported makevn runner architecture\n' >&2; exit 1 ;;
esac
read -r version checksum < <(python3 - "$script_dir/makevn.lock.json" "$target" <<'PY'
import json
import sys
lock = json.load(open(sys.argv[1]))
print(lock["version"], lock["sha256"][sys.argv[2]])
PY
)
work_dir="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/makevn.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT
archive="makevn-${version}-${target}.tar.gz"
curl --fail --silent --show-error --location \
  --retry 4 --retry-delay 2 --retry-max-time 120 \
  --connect-timeout 15 --max-time 120 \
  "https://github.com/antonillos/makevn/releases/download/${version}/${archive}" \
  --output "$work_dir/$archive"
# The reviewed digest lives in git; never trust a checksum fetched alongside a corrupted archive.
printf '%s  %s\n' "$checksum" "$archive" > "$work_dir/SHA256SUMS"
(cd "$work_dir" && shasum -a 256 -c SHA256SUMS)

# Extract only after verification, to a new job-local directory (no overwrite of a developer install).
prefix="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/makevn-installed.XXXXXX")"
tar -xzf "$work_dir/$archive" -C "$prefix" --strip-components=1
"$prefix/bin/makevn" --version
if [[ -n "${GITHUB_PATH:-}" ]]; then
  printf '%s/bin\n' "$prefix" >> "$GITHUB_PATH"
fi
printf 'makevn installed at %s/bin\n' "$prefix"
