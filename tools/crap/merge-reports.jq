def coverage_value: (.coverage_percent // .coverage);

def suggestion:
  if .crap == null then
    "Verify that this function is included in coverage and add a focused test."
  elif (.complexity // .cyclomatic) >= 8 and (coverage_value // 0) < 60 then
    "Split the control flow into focused helpers and add tests for each branch."
  elif (.complexity // .cyclomatic) >= 8 then
    "Simplify the control flow; high coverage does not remove structural complexity."
  elif (coverage_value // 0) < 60 then
    "Add focused tests for the uncovered branches and error paths."
  else
    "Keep the existing coverage and consider a small simplification if this function changes."
  end;

def rust_entries:
  ($rust.entries // []) | map({
    language: "rust",
    file: .file,
    line: .line,
    end_line: null,
    symbol: .function,
    complexity: .cyclomatic,
    coverage_percent: .coverage,
    crap: .crap,
    status: (if .crap == null then "missing-coverage" else "measured" end),
    uncovered_ranges: ([.uncovered[]? | "\(.start)-\(.end)"]),
    remediation: suggestion
  });

def java_entries:
  ($java.entries // []) | map({
    language: "java",
    file: .file,
    line: .line,
    end_line: .end_line,
    symbol: ((.class // "") + "#" + .method),
    complexity: .complexity,
    coverage_percent: .coverage_percent,
    crap: .crap,
    status: .status,
    uncovered_ranges: [],
    remediation: suggestion
  });

{
  schema: "https://github.com/antonillos/safeselect/crap-report-v1.json",
  formula: "CC^2 * (1 - coverage)^3 + CC",
  threshold: $threshold,
  entries: ((rust_entries) + (java_entries)
    | sort_by([-(.crap // -1), .language, .file, .line, .symbol])),
  diagnostics: {
    rust_functions: (($rust.entries // []) | length),
    java_methods: (($java.entries // []) | length),
    missing_coverage: ((($rust.entries // []) + ($java.entries // [])) | map(select(.crap == null)) | length)
  }
}
