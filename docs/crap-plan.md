# SafeSelect CRAP Analysis Plan

## Goal

Add a local, deterministic, report-only CRAP analysis for all production Rust
and Java code. The agent should only invoke one command and read a compact
exit summary; all coverage collection, complexity analysis, correlation,
scoring, and report generation must happen locally.

The first release must not fail CI because of CRAP findings.

## Design decisions

- Use the standard formula:

  `CRAP = CC^2 * (1 - coverage)^3 + CC`

- Use `cargo-crap` for Rust instead of reimplementing Rust complexity,
  coverage correlation, path normalization, baseline handling, and reporting.
- Use `crap4java` as the first Java implementation candidate because it
  already combines JaCoCo coverage with AST-based Java complexity analysis.
  Keep a direct JaCoCo adapter as the fallback if its output contract cannot
  be extended safely.
- Use `makevn` as the only Java/Maven execution interface. The CRAP scripts
  must not invoke `mvn` or `mvnw` directly.
- Use POSIX shell as the orchestration layer. Do not add Python for the first
  implementation.
- Keep raw machine-readable reports under `target/crap/`; print only a
  bounded summary to stdout.
- Analyze production code by default. Exclude Rust tests, examples, benches,
  generated code, and Java test sources.
- Keep this phase report-only. Threshold and regression failures are recorded
  in the report but do not fail the Verify workflow.

## Why cargo-crap is useful

`cargo-crap` already provides the Rust half of the required functionality:

- consumes LCOV produced by `cargo llvm-cov` or `cargo tarpaulin`;
- calculates per-function CRAP and cyclomatic complexity;
- reports source locations and uncovered line ranges;
- supports JSON, Markdown, SARIF, GitHub annotations, and PR-comment output;
- supports exclusions, missing-coverage policies, thresholds, and baselines;
- detects CRAP regressions against a previous JSON report;
- returns distinct exit codes for a completed gate versus an analysis error.

Therefore we should not build a Rust parser or Rust coverage joiner. The
wrapper should invoke `cargo llvm-cov` and `cargo crap`, then normalize the
result into the cross-language report.

## Why crap4java is useful

`crap4java` is a strong fit for the current sidecar because the sidecar is a
Maven module and the tool already performs the required Java analysis pipeline:

- runs Maven tests with JaCoCo;
- reads JaCoCo XML;
- parses Java methods using compiler tree APIs;
- computes cyclomatic complexity from syntax;
- applies the standard CRAP formula;
- reports method source locations and sorts by worst score first.

The upstream tool was not a complete drop-in for the final report or execution
workflow: its documented contract was Maven-only, used a fixed threshold of
`8.0`, and did not define machine-readable output. The maintained fork now
provides the required JSON and external-report modes, so the shell wrapper does
not need to parse an unstable human table.

The upstream tool launches Maven internally. The fork now supports both
`--jacoco-xml` for externally generated coverage and `--build-tool makevn` for
native coverage execution through `makevn verify-ut-coverage --compact`.
Coverage generation remains explicit and the default Maven mode remains
backward compatible.

Preferred Java implementation order:

1. maintain the upstream-compatible fork at
   `https://github.com/antonillos/crap4java`;
2. use its JSON and external-report modes from the local shell wrapper;
3. normalize that JSON into the common report schema;
4. keep the fork's README explicit about the differences from upstream.

The fork currently has these implementation commits:

- `e410496` (`feat: add machine-readable report mode`);
- `1d049a2` (`feat: support makevn coverage execution`).

## Proposed command interface

```text
./tools/crap/run.sh                 # orchestrate the independent analyzers
./tools/crap/run.sh --summary       # bounded stdout summary only
./tools/crap/rust-crap.sh           # Rust-only report
./tools/crap/java-crap.sh           # Java-only report
./tools/crap/report.sh R.json J.json # language-agnostic merge/render
./tools/crap/run.sh --help
```

The command must always write:

```text
target/crap/
  rust-coverage.lcov
  rust-report.json
  java-jacoco.xml
  java-report.json
  report.json
  report.md
  report.sarif
  summary.txt
```

The command must print the report paths and final status, never the complete
JSON/XML/LCOV contents.

## Report contract

Every method/function entry must contain:

```json
{
  "language": "rust|java",
  "file": "src/security.rs",
  "line": 142,
  "symbol": "validate_query",
  "complexity": 8,
  "coverage_percent": 42.0,
  "crap": 27.1,
  "status": "ok|warning|missing-coverage|excluded",
  "uncovered_ranges": ["151-158", "171"]
}
```

For every actionable finding, the Markdown report must show:

1. exact file and line;
2. symbol/method name;
3. complexity and coverage;
4. CRAP score and reason for the score;
5. uncovered line ranges;
6. a concrete remediation suggestion;
7. recommended tests or test location;
8. a stable command to rerun the relevant analysis;
9. whether the finding is new, unchanged, improved, or regressed.

Suggested remediation text:

- high complexity and low coverage: split the function and add focused tests
  for each branch;
- high complexity and high coverage: simplify control flow; tests are not a
  substitute for reducing complexity;
- low complexity and low coverage: add a focused unit test;
- missing coverage: verify that the function is included in the coverage
  target and that the relevant test path executes it.

The report must include a top-level `diagnostics` section containing analyzed,
covered, matched, excluded, and missing files/functions. A path mismatch must
be an explicit diagnostic, never silently interpreted as 0% coverage.

## Implementation phases

### Phase 1 — Tool and scope discovery

- Confirm the supported Rust toolchain satisfies the selected `cargo-crap`
  release.
- Confirm `cargo llvm-cov`, `cargo crap`, makevn, JaCoCo, `jq`, and an XML
  parser are available locally and in Ubuntu CI.
- Define the production scope explicitly for `src/` and
  `sidecar/src/main/java/`.
- Record exclusions and missing-coverage policy in a checked-in configuration.

Acceptance: the command can validate dependencies and prints actionable
installation instructions without running analysis.

### Phase 2 — Rust report-only adapter

- Generate LCOV with `cargo llvm-cov`.
- Run `cargo crap --format json` with stable sorting and configured exclusions.
- Preserve `uncovered` ranges and diagnostics from the Rust report.
- Convert Rust entries to the common report schema.
- Do not use `--fail-above` or `--fail-regression` yet.

Acceptance: Rust-only analysis produces JSON, Markdown, SARIF, and a bounded
summary without model-side processing.

### Phase 3 — Java coverage and complexity adapter

- Configure JaCoCo activation and report layout through the repository's
  makevn configuration/profile; do not call Maven directly from the CRAP
  scripts.
- Run the existing Java tests with JaCoCo enabled through makevn, for example
  `makevn clean verify-ut-coverage` once the profile is configured.
- Generate XML coverage for `sidecar/src/main/java`.
- Use the fork's AST analyzer and JSON output to obtain method complexity and
  source locations.
- Correlate methods using class, method name, descriptor, and source line;
  never rely on method name alone because overloads are possible.
- Emit uncovered source line ranges from JaCoCo line counters.

Acceptance: Java-only analysis identifies every concrete production method or
reports an explicit, bounded attribution diagnostic.

### Phase 4 — Cross-language report

- Merge Rust and Java entries into `report.json`.
- Sort findings by CRAP descending, then language, file, line, and symbol.
- Generate `report.md` with sections for summary, critical findings,
  language breakdown, diagnostics, and exact remediation commands.
- Generate SARIF locations for findings that exceed the configured report
  threshold.
- Generate `summary.txt` containing only counts, worst findings, paths, and
  status.

Acceptance: a developer can open `report.md` and fix a finding without
rerunning the analyzer to discover missing context.

### Phase 5 — Local tests for the analyzer

Add fixture-based tests for:

- formula calculations;
- 0%, partial, and 100% coverage;
- overloaded Java methods;
- Rust and Java path normalization;
- missing coverage data;
- generated/test exclusions;
- uncovered line range formatting;
- deterministic ordering;
- malformed coverage reports;
- tool failures and exit-code propagation.

Acceptance: analyzer tests run locally without database services or secrets.

### Phase 6 — Report-only Verify integration

- Add a separate `CRAP (report-only)` job to `.github/workflows/verify.yml`.
- Upload `target/crap/` as a workflow artifact.
- Keep the job successful when findings exceed the provisional threshold.
- Fail only when the analyzer itself cannot complete or produces invalid
  output.
- Do not add PR comments in the first iteration; use artifacts first to avoid
  noise and excessive permissions.

Acceptance: every Verify run publishes a complete actionable report and does
not block merges because of existing CRAP debt.

### Phase 7 — Future enforcement, not part of this phase

- Commit or publish a baseline generated from `develop`.
- Add changed-code regression detection.
- Introduce language-specific provisional thresholds only after reviewing the
  first reports.
- Enable CI failure for new regressions before considering absolute thresholds.

## Shell responsibilities and SOLID boundaries

Shell is appropriate for:

- dependency checks;
- invoking coverage/test tools;
- creating the output directory;
- collecting exit statuses;
- calling `jq`, `awk`, and XML tooling;
- writing bounded summaries;
- returning a stable final status.

`rust-crap.sh` owns only Rust coverage and Rust CRAP invocation.
`java-crap.sh` owns only makevn, JaCoCo, and Java CRAP invocation.
`report.sh` owns only schema normalization, Markdown/SARIF generation, and
summary output; it does not execute language tools.
`run.sh` is only an orchestrator and does not know language-specific details.

No script should implement Java parsing, XML parsing, or Rust complexity
analysis itself. Those responsibilities belong to JaCoCo, `cargo-crap`, and
the Java analyzer fork.

## Required local validation

Before CI integration:

```text
cargo fmt --check
cargo test
makevn test
./tools/crap/run.sh --summary
test -s target/crap/report.json
test -s target/crap/report.md
test -s target/crap/report.sarif
```

The local command must work on macOS and Ubuntu, avoid network access after
tools are installed, and never expose credentials or database payloads.
