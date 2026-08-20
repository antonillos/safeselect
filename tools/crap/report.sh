#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(CDPATH= cd -- "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="${ROOT_DIR}/target/crap"
RUST_REPORT="${1:?Rust report path is required}"
JAVA_REPORT="${2:?Java report path is required}"
THRESHOLD="${3:-8.0}"

command -v jq >/dev/null 2>&1 || { printf 'Error: jq is required.\n' >&2; exit 2; }
mkdir -p "${OUT_DIR}"

jq -n \
  --argjson rust "$(cat "${RUST_REPORT}")" \
  --argjson java "$(cat "${JAVA_REPORT}")" \
  --argjson threshold "${THRESHOLD}" \
  -f "${ROOT_DIR}/tools/crap/merge-reports.jq" \
  >"${OUT_DIR}/report.json"

jq -n --slurpfile report "${OUT_DIR}/report.json" '
  ($report[0].entries) as $entries |
  ($entries | map(select(.crap != null and .crap > $report[0].threshold)) | length) as $warnings |
  {
    schemaVersion: 1,
    label: "CRAP",
    message: (($warnings | tostring) + " warnings"),
    color: (if $warnings == 0 then "brightgreen" elif $warnings <= 50 then "yellow" elif $warnings <= 100 then "orange" else "red" end),
    cacheSeconds: 300
  }
' >"${OUT_DIR}/badge.json"

jq -r -f "${ROOT_DIR}/tools/crap/render-report.jq" "${OUT_DIR}/report.json" >"${OUT_DIR}/report.md"

jq -n --slurpfile report "${OUT_DIR}/report.json" '
  {
    version: "2.1.0",
    "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
    runs: [{
      tool: {driver: {name: "cross-language-crap", informationUri: "https://github.com/antonillos/crap4java"}},
      results: ([$report[0].entries[] | select(.crap != null and .crap > $report[0].threshold) | {
        ruleId: "CRAP",
        level: "warning",
        message: {text: (.remediation + " CRAP=" + (.crap | tostring) + ", CC=" + (.complexity | tostring) + ", coverage=" + ((.coverage_percent // 0) | tostring) + "%.")},
        locations: [{physicalLocation: {artifactLocation: {uri: .file}, region: {startLine: .line}}}]
      }])
    }]
  }
' >"${OUT_DIR}/report.sarif"

jq -r '
  (.threshold) as $threshold |
  (.entries) as $entries |
  ($entries | map(select(.crap != null and .crap > $threshold))) as $warnings |
  ($entries | group_by(.language) | map({language: .[0].language, total: length, warnings: (map(select(.crap != null and .crap > $threshold)) | length)})) as $languages |
  "CRAP report completed",
  ("Functions: " + (($entries | length) | tostring)),
  ("Warnings: " + (($warnings | length) | tostring)),
  ("Missing coverage: " + (($entries | map(select(.coverage_percent == null)) | length) | tostring)),
  ("By language: " + ($languages | map(.language + "=" + (.total | tostring) + " (warnings=" + (.warnings | tostring) + ")") | join(", "))),
  (if ($warnings | length) > 0 then
     ("Top finding: " + ($warnings | sort_by(-.crap) | .[0] | (.file + ":" + (.line | tostring) + " " + .symbol + " CRAP=" + ((.crap * 100 | round) / 100 | tostring))))
   else "Top finding: none" end),
  "Report: target/crap/report.md",
  "JSON: target/crap/report.json"
' "${OUT_DIR}/report.json" | tee "${OUT_DIR}/summary.txt"
