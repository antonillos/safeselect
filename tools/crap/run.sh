#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(CDPATH= cd -- "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="${ROOT_DIR}/target/crap"
SUMMARY_ONLY=false

usage() {
  cat <<'USAGE'
Usage: tools/crap/run.sh [--summary]

Run separate Rust and Java report-only CRAP analyzers and combine their local
machine-readable reports. The language analyzers do not depend on each other.
USAGE
}

for arg in "$@"; do
  case "${arg}" in
    --summary) SUMMARY_ONLY=true ;;
    --help|-h) usage; exit 0 ;;
    *) printf 'Error: unsupported argument: %s\n' "${arg}" >&2; usage >&2; exit 2 ;;
  esac
done

command -v jq >/dev/null 2>&1 || { printf 'Error: jq is required.\n' >&2; exit 2; }
mkdir -p "${OUT_DIR}"
rm -f "${OUT_DIR}"/*

"${ROOT_DIR}/tools/crap/rust-crap.sh"
"${ROOT_DIR}/tools/crap/java-crap.sh"
"${ROOT_DIR}/tools/crap/report.sh" \
  "${OUT_DIR}/rust-report.json" \
  "${OUT_DIR}/java-report.json"

if [[ "${SUMMARY_ONLY}" != true ]]; then
  printf '%s\n' 'Artifacts:'
  printf '  %s\n' \
    "${OUT_DIR}/report.json" \
    "${OUT_DIR}/report.md" \
    "${OUT_DIR}/report.sarif" \
    "${OUT_DIR}/rust-report.json" \
    "${OUT_DIR}/java-report.json"
fi
