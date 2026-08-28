#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(CDPATH= cd -- "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="${ROOT_DIR}/target/crap"
SUMMARY_ONLY=false
RATCHET_MAX_WARNINGS=""

usage() {
  cat <<'USAGE'
Usage: tools/crap/run.sh [--summary] [--ratchet MAX_WARNINGS]

Run separate Rust and Java report-only CRAP analyzers and combine their local
machine-readable reports. The language analyzers do not depend on each other.
With --ratchet, the wrapper fails if the warning count exceeds MAX_WARNINGS.
USAGE
}

for arg in "$@"; do
  case "${arg}" in
    --summary) SUMMARY_ONLY=true ;;
    --ratchet) : ;;
    --help|-h) usage; exit 0 ;;
    *)
      if [[ "${previous_arg:-}" == "--ratchet" ]]; then
        RATCHET_MAX_WARNINGS="${arg}"
      else
        printf 'Error: unsupported argument: %s\n' "${arg}" >&2; usage >&2; exit 2
      fi
      ;;
  esac
  previous_arg="${arg}"
done

if [[ "${previous_arg:-}" == "--ratchet" ]]; then
  printf 'Error: --ratchet requires a maximum warning count.\n' >&2
  exit 2
fi
if [[ -n "${RATCHET_MAX_WARNINGS}" && ! "${RATCHET_MAX_WARNINGS}" =~ ^[0-9]+$ ]]; then
  printf 'Error: --ratchet requires a non-negative integer.\n' >&2
  exit 2
fi

command -v jq >/dev/null 2>&1 || { printf 'Error: jq is required.\n' >&2; exit 2; }
mkdir -p "${OUT_DIR}"
rm -f "${OUT_DIR}"/*

"${ROOT_DIR}/tools/crap/rust-crap.sh"
"${ROOT_DIR}/tools/crap/java-crap.sh"
# Keep report.sh's default score threshold; pass only the wrapper's count limit.
"${ROOT_DIR}/tools/crap/report.sh" \
  "${OUT_DIR}/rust-report.json" \
  "${OUT_DIR}/java-report.json" \
  "" "${RATCHET_MAX_WARNINGS}"

if [[ -n "${RATCHET_MAX_WARNINGS}" ]]; then
  warnings="$(jq '(.threshold) as $threshold | [.entries[] | select(.crap != null and .crap > $threshold)] | length' "${OUT_DIR}/report.json")"
  if (( warnings > RATCHET_MAX_WARNINGS )); then
    printf 'CRAP ratchet failed: %s warnings exceed maximum %s.\n' \
      "${warnings}" "${RATCHET_MAX_WARNINGS}" >&2
    exit 1
  fi
  printf 'CRAP ratchet passed: %s/%s warnings.\n' "${warnings}" "${RATCHET_MAX_WARNINGS}"
fi

if [[ "${SUMMARY_ONLY}" != true ]]; then
  printf '%s\n' 'Artifacts:'
  printf '  %s\n' \
    "${OUT_DIR}/report.json" \
    "${OUT_DIR}/report.md" \
    "${OUT_DIR}/report.sarif" \
    "${OUT_DIR}/rust-report.json" \
    "${OUT_DIR}/java-report.json"
fi
