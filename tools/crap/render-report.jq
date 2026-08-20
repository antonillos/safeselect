def score:
  if . == null then "N/A" else ((. * 100 | round) / 100 | tostring) end;

def percentage:
  if . == null then "N/A" else ((. * 100 | round) / 100 | tostring) + "%" end;

def cell:
  tostring | gsub("\\|"; "\\\\|") | gsub("\n"; " ");

(.threshold) as $threshold |
(.entries) as $entries |
($entries | map(select(.crap != null and .crap > $threshold)) | sort_by(-.crap)) as $warnings |
($entries | group_by(.language) | map({
  language: .[0].language,
  total: length,
  warnings: (map(select(.crap != null and .crap > $threshold)) | length),
  missing: (map(select(.coverage_percent == null)) | length)
})) as $languages |
[
  "# CRAP Report",
  "",
  "**Status:** REPORT-ONLY — findings do not block CI.",
  ("**Threshold:** CRAP > " + ($threshold | tostring)),
  "",
  "## Executive summary",
  "",
  "| Language | Functions/methods | Warnings | Missing coverage |",
  "|---|---:|---:|---:|",
  ($languages[] | "| " + .language + " | " + (.total | tostring) + " | " + (.warnings | tostring) + " | " + (.missing | tostring) + " |"),
  ("| **Total** | **" + ($entries | length | tostring) + "** | **" + ($warnings | length | tostring) + "** | **" + ($entries | map(select(.coverage_percent == null)) | length | tostring) + "** |"),
  "",
  "## How to apply the findings",
  "",
  "1. Start with the first rows in the priority table; they have the highest CRAP risk.",
  "2. Open the exact file and line shown in **Location**.",
  "3. Apply the suggested remediation: split complex control flow and add tests for uncovered branches/error paths.",
  "4. Re-run `./tools/crap/run.sh --summary` and verify the finding improves.",
  "",
  "## Priority findings",
  "",
  "| # | Language | Location | Symbol | CRAP | CC | Coverage | Recommended action |",
  "|---:|---|---|---|---:|---:|---:|---|",
  (if ($warnings | length) == 0 then
     "| — | — | — | No findings exceed the threshold | — | — | — | — |"
   else
     ($warnings[0:25] | to_entries[] | "| " + ((.key + 1) | tostring) + " | " + .value.language + " | `" + .value.file + ":" + (.value.line | tostring) + "` | " + (.value.symbol | cell) + " | " + (.value.crap | score) + " | " + (.value.complexity | score) + " | " + (.value.coverage_percent | percentage) + " | " + (.value.remediation | cell) + " |")
   end),
  "",
  (if ($warnings | length) > 25 then
     ("Showing the top 25 of " + ($warnings | length | tostring) + " warnings. See `report.json` for the complete machine-readable list.")
   else "The table contains all threshold warnings." end),
  "",
  "## Artifacts",
  "",
  "- `report.json` — complete normalized report with exact source locations and remediation fields.",
  "- `report.sarif` — findings for code-scanning-compatible tools.",
  "- `rust-report.json` and `java-report.json` — language-specific source reports."
] | join("\n")
