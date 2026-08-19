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

jq -r '
  (.threshold) as $threshold |
  "# CRAP Report\n",
  "Generated locally; report-only mode does not block CI.\n",
  "## Summary\n",
  ("- Functions/methods: " + ((.entries | length) | tostring)),
  ("- Threshold warnings: " + (([.entries[] | select(.crap != null and .crap > $threshold)] | length) | tostring)),
  ("- Missing coverage: " + (([.entries[] | select(.coverage_percent == null)] | length) | tostring)),
  "\n## Actionable findings\n",
  (if ([.entries[] | select(.crap != null and .crap > $threshold)] | length) == 0 then
     "No findings exceed the report threshold."
   else
     ([.entries[] | select(.crap != null and .crap > $threshold)] | map(
       "- **" + (.language + " " + .symbol) + "** — " + .file + ":" + (.line | tostring) +
       " — CRAP " + (.crap | tostring) + ", CC " + (.complexity | tostring) +
       ", coverage " + ((.coverage_percent // 0) | tostring) + "%.\n  - " + .remediation
     ) | join("\n"))
   end)
' "${OUT_DIR}/report.json" >"${OUT_DIR}/report.md"

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

jq -r '(.threshold) as $threshold | "CRAP report completed", ("Functions: " + ((.entries | length) | tostring)), ("Warnings: " + (([.entries[] | select(.crap != null and .crap > $threshold)] | length) | tostring)), ("Missing coverage: " + (([.entries[] | select(.coverage_percent == null)] | length) | tostring)), "Report: target/crap/report.md", "JSON: target/crap/report.json"' "${OUT_DIR}/report.json" | tee "${OUT_DIR}/summary.txt"
