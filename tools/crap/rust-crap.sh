#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(CDPATH= cd -- "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="${ROOT_DIR}/target/crap"

require_command() {
  command -v "$1" >/dev/null 2>&1 || {
    printf 'Error: required command not found: %s\n' "$1" >&2
    exit 2
  }
}

require_command cargo
require_command cargo-llvm-cov
require_command jq
if ! cargo crap --help >/dev/null 2>&1; then
  printf 'Error: cargo-crap is not installed.\n' >&2
  exit 2
fi

mkdir -p "${OUT_DIR}"
if ! cargo llvm-cov --lcov --output-path "${OUT_DIR}/rust-coverage.lcov" >"${OUT_DIR}/rust-coverage.log" 2>&1; then
  tail -n 80 "${OUT_DIR}/rust-coverage.log" >&2
  exit 1
fi
if ! cargo crap --lcov "${OUT_DIR}/rust-coverage.lcov" --workspace --format json --output "${OUT_DIR}/rust-report.json" >"${OUT_DIR}/rust-crap.log" 2>&1; then
  tail -n 80 "${OUT_DIR}/rust-crap.log" >&2
  exit 1
fi
jq --arg root "${ROOT_DIR}/" \
  '.entries |= map(.file |= if startswith($root) then .[($root | length):] else . end)' \
  "${OUT_DIR}/rust-report.json" >"${OUT_DIR}/rust-report.normalized.json"
mv "${OUT_DIR}/rust-report.normalized.json" "${OUT_DIR}/rust-report.json"
jq -e '.entries and (.entries | type == "array")' "${OUT_DIR}/rust-report.json" >/dev/null
