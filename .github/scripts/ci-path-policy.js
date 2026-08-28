"use strict";

const DOC_PATH = /^(README(?:\.[^/]+)?|docs\/|demo\/(?:README\.md|recordings\/|[^/]+\.tape$))/;
// These helpers validate only the marketing site. Other tools/ci paths remain
// unknown (full verification), including release tooling and shared validators.
const WEBSITE_HELPERS = new Set([
  "tools/ci/validate_site.py",
  "tools/ci/test_site_validation.py",
]);
const isWebsitePath = (path) => path.startsWith("site/") || WEBSITE_HELPERS.has(path);
const POSTGRES_PATH = /^(sidecar\/|src\/(?:sidecar|config\/driver)\.rs$|tests\/(?:integration\.rs|security_suite\/real_postgres\.rs|smoke_suite\/(?:postgres|reconnect)\.rs))/;
const MONGODB_PATH = /^tests\/(?:security_suite\/real_mongodb\.rs|smoke_suite\/mongodb\.rs)/;
const SHARED_TEST_PATH = /^tests\//;
const SHARED_RUST_PATH = /^src\//;
const SENSITIVE_PATH = /^(\.github\/|packaging\/|scripts\/|tools\/security\/|\.makevn\/|(?:Cargo\.(?:toml|lock)|deny\.toml|docker-compose(?:\.[^/]+)?\.ya?ml|Makefile)$)/;

function fullPolicy(reason) {
  return {
    profile: reason,
    docs: false,
    crap: true,
    unit: true,
    postgres: true,
    mongodb: true,
    security: true,
    realIntegration: true,
  };
}

function classifyPaths(paths) {
  if (!Array.isArray(paths) || paths.length === 0) return fullPolicy("unknown");
  if (paths.some((path) => SENSITIVE_PATH.test(path))) return fullPolicy("sensitive");

  const flags = {
    docs: paths.every((path) => DOC_PATH.test(path)),
    website: paths.every((path) => DOC_PATH.test(path) || isWebsitePath(path)),
    postgres: paths.some((path) => POSTGRES_PATH.test(path)),
    mongodb: paths.some((path) => MONGODB_PATH.test(path)),
    sharedTests: paths.some((path) => SHARED_TEST_PATH.test(path) && !POSTGRES_PATH.test(path) && !MONGODB_PATH.test(path)),
    sharedRust: paths.some((path) => SHARED_RUST_PATH.test(path) && !POSTGRES_PATH.test(path)),
  };

  // Website validation is always required by the Verify workflow. This profile
  // selects documentation checks without unrelated Rust/Java/database suites.
  if (flags.docs || flags.website) {
    return {
      profile: flags.docs ? "docs" : "website",
      docs: true,
      crap: false,
      unit: false,
      postgres: false,
      mongodb: false,
      security: false,
      realIntegration: false,
    };
  }

  if (paths.some((path) => !DOC_PATH.test(path) && !isWebsitePath(path) && !POSTGRES_PATH.test(path) && !MONGODB_PATH.test(path) && !SHARED_TEST_PATH.test(path) && !SHARED_RUST_PATH.test(path))) {
    return fullPolicy("unknown");
  }

  if (flags.sharedTests) {
    flags.postgres = true;
    flags.mongodb = true;
  }

  const backend = flags.postgres || flags.mongodb;
  return {
    profile: backend ? (flags.postgres && flags.mongodb ? "both-backends" : flags.postgres ? "postgres" : "mongodb") : "shared-rust",
    docs: false,
    crap: true,
    unit: true,
    postgres: flags.postgres,
    mongodb: flags.mongodb,
    security: false,
    realIntegration: backend,
  };
}

module.exports = { classifyPaths, fullPolicy };
