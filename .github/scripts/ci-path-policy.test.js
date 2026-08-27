"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { resolve } = require("node:path");
const { spawnSync } = require("node:child_process");
const { classifyPaths } = require("./ci-path-policy");

function expected(paths, profile, enabled) {
  const actual = classifyPaths(paths);
  assert.equal(actual.profile, profile);
  for (const name of ["docs", "crap", "unit", "postgres", "mongodb", "security", "realIntegration"]) {
    assert.equal(actual[name], enabled.includes(name), `${name} for ${(paths || []).join(", ")}`);
  }
}

test("classifies documentation-only changes as lightweight", () =>
  expected(["README.md", "docs/security-proof.md"], "docs", ["docs"]));

test("classifies VHS tapes as documentation-only", () =>
  expected(["demo/safeselect-proof.tape", "docs/recordings/safeselect-proof.gif"], "docs", ["docs"]));

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
  expected([".github/scripts/ci-path-policy.js"], "sensitive", ["crap", "unit", "postgres", "mongodb", "security", "realIntegration"]);
  expected(["new-top-level-file.txt"], "unknown", ["crap", "unit", "postgres", "mongodb", "security", "realIntegration"]);
  expected(["docs/security-proof.md", "src/mcp.rs"], "shared-rust", ["crap", "unit"]);
});

const WEBSITE_PR = [
  "docs/positioning.md",
  "site/public/googled7be89f4207cbfe7.html",
  "site/scripts/export.tsx",
  "tools/ci/test_site_validation.py",
  "tools/ci/validate_site.py",
];
const FULL = ["crap", "unit", "postgres", "mongodb", "security", "realIntegration"];

test("classifies the exact Google verification PR as website-only", () =>
  expected(WEBSITE_PR, "website", ["docs"]));

test("classifies website assets, code, dependencies and exact helpers as website-only", () => {
  for (const path of ["site/app/page.tsx", "site/public/og.png", "site/package.json",
    "site/package-lock.json", "site/vite.config.ts", "site/.openai/hosting.json",
    "tools/ci/validate_site.py", "tools/ci/test_site_validation.py"]) {
    expected([path], "website", ["docs"]);
    expected(["README.md", path], "website", ["docs"]);
  }
});

test("website paths never hide unknown or sensitive changes", () => {
  for (const path of ["tools/ci/release.py", "tools/ci/validate_docs.py",
    "tools/ci/test_site_validation.py.bak", "tools/ci/new_tool.py", "site-other/file.txt"]) {
    expected([...WEBSITE_PR, path], "unknown", FULL);
  }
  for (const path of [".github/workflows/verify.yml", ".github/scripts/ci-path-policy.js",
    "Cargo.toml", "Cargo.lock", "packaging/homebrew/safeselect.rb", "tools/security/validate_manifest.py"]) {
    expected([...WEBSITE_PR, path], "sensitive", FULL);
  }
});

test("mixed website changes retain the product's required jobs", () => {
  const cases = [
    ["src/mcp.rs", "shared-rust", ["crap", "unit"]],
    ["sidecar/src/main/java/com/safeselect/Main.java", "postgres", ["crap", "unit", "postgres", "realIntegration"]],
    ["tests/security_suite/real_mongodb.rs", "mongodb", ["crap", "unit", "mongodb", "realIntegration"]],
    ["tests/security.rs", "both-backends", ["crap", "unit", "postgres", "mongodb", "realIntegration"]],
  ];
  for (const [path, profile, jobs] of cases) {
    expected([...WEBSITE_PR, path], profile, jobs);
    expected([path, ...WEBSITE_PR], profile, jobs);
  }
});

test("empty or unavailable path lists still require full verification", () => {
  for (const paths of [[], null, undefined]) expected(paths, "unknown", FULL);
});

// Exercise the actual workflow gate, not a second implementation of its rules.
const workflow = readFileSync(resolve(__dirname, "../workflows/verify.yml"), "utf8");
const jobs = Object.fromEntries([...workflow.split("jobs:\n")[1]
  .matchAll(/^  ([\w-]+):\n([\s\S]*?)(?=^  [\w-]+:|(?![\s\S]))/gm)]
  .map((match) => [match[1], match[2]]));

test("lightweight profile retains all website checks and mandatory Verify dependency", () => {
  const website = jobs.website;
  assert.ok(website);
  assert.doesNotMatch(website, /^    if:/m, "website job must not become conditional");
  for (const command of ["npm run build:pages", "npm run typecheck", "npm run lint",
    "python3 tools/ci/validate_docs.py", "python3 tools/ci/validate_site.py", "test_site_validation.py"]) {
    assert.ok(website.includes(command), `missing ${command}`);
  }
  assert.match(jobs.verify, /needs: \[[^\n]*website/);
  assert.ok(jobs.verify.includes("WEBSITE_RESULT: ${{ needs.website.result }}"));
});

test("website profile can pass Verify only with successful docs and website", () => {
  const script = jobs.verify.split("run: |\n")[1];
  assert.ok(script);
  const policy = classifyPaths(WEBSITE_PR);
  const keys = { DOCS: "docs", CRAP: "crap", UNIT: "unit", POSTGRES: "postgres",
    MONGODB: "mongodb", SECURITY: "security", REAL_INTEGRATION: "realIntegration" };
  const env = { ...process.env, WEBSITE_RESULT: "success" };
  for (const [name, key] of Object.entries(keys)) {
    env[`${name}_REQUIRED`] = String(policy[key]);
    env[`${name}_RESULT`] = policy[key] ? "success" : "skipped";
  }
  assert.equal(spawnSync("bash", ["-c", script], { env }).status, 0);
  for (const name of ["DOCS", "WEBSITE"]) {
    for (const result of ["failure", "cancelled", "skipped", ""]) {
      assert.equal(spawnSync("bash", ["-c", script], {
        env: { ...env, [`${name}_RESULT`]: result },
      }).status, 1, `${name}=${result} must fail Verify`);
    }
  }
});
