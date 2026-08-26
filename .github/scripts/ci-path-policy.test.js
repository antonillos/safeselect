"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { classifyPaths } = require("./ci-path-policy");

function expected(paths, profile, enabled) {
  const actual = classifyPaths(paths);
  assert.equal(actual.profile, profile);
  for (const name of ["docs", "crap", "unit", "postgres", "mongodb", "security", "realIntegration"]) {
    assert.equal(actual[name], enabled.includes(name), `${name} for ${paths.join(", ")}`);
  }
}

test("classifies documentation-only changes as lightweight", () =>
  expected(["README.md", "docs/security-proof.md"], "docs", ["docs"]));

test("classifies VHS tapes as documentation-only", () =>
  expected(["demo/safeselect-proof.tape", "docs/recordings/safeselect-proof.gif"], "docs", ["docs"]));

test("classifies CI path policy changes without backend integration", () =>
  expected([".github/scripts/ci-path-policy.js", "README.md"], "ci-policy", ["docs", "crap", "unit"]));

test("classifies shared Rust as CRAP and unit only", () =>
  expected(["src/mcp.rs"], "shared-rust", ["crap", "unit"]));
test("classifies PostgreSQL, JDBC, and sidecar changes", () =>
  expected(["sidecar/src/main/java/com/safeselect/Main.java"], "postgres", ["crap", "unit", "postgres", "realIntegration"]));
test("classifies MongoDB fixtures", () =>
  expected(["tests/smoke_suite/mongodb.rs"], "mongodb", ["crap", "unit", "mongodb", "realIntegration"]));
test("classifies shared tests as both backends", () =>
  expected(["tests/security.rs"], "both-backends", ["crap", "unit", "postgres", "mongodb", "realIntegration"]));
test("classifies sensitive, unknown, and mixed paths conservatively", () => {
  expected([".github/workflows/verify.yml"], "sensitive", ["crap", "unit", "postgres", "mongodb", "security", "realIntegration"]);
  expected(["new-top-level-file.txt"], "unknown", ["crap", "unit", "postgres", "mongodb", "security", "realIntegration"]);
  expected(["docs/security-proof.md", "src/mcp.rs"], "shared-rust", ["crap", "unit"]);
});
