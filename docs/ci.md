# Verification profiles

The required **Verify** check aggregates the jobs selected by
`.github/scripts/ci-path-policy.js`. It does not pass a selected job that failed,
was cancelled or was skipped. **Website and positioning** is always required;
it builds the static site, checks TypeScript/lint, validates Markdown, links,
metadata and assets, and runs the website validator tests.

## Website-only pull requests

A PR containing only the following paths uses the `website` profile:

- `site/` (including its source, assets, configuration and npm lockfile).
- `tools/ci/validate_site.py`.
- `tools/ci/test_site_validation.py`.
- Optionally, existing documentation paths such as `README.md` and `docs/`.

It requires **Documentation**, **Website and positioning** and the final
**Verify** gate, without Rust/Java compilation, CRAP or database integration.
The two Python helpers are an exact allowlist, not an exemption for `tools/ci/`.
Changes to shared documentation validation or release tooling still use full
verification unless separately classified.

Mixing website changes with product changes preserves that product's normal
checks. For example, a sidecar change still requires PostgreSQL integration;
shared tests require both backends. Sensitive paths, including `.github/`, and
unknown paths retain full verification even when combined with website files.
Consequently, a PR changing this classification policy must itself run the full
suite; the optimization must not exempt its own implementation from review.

## Java CRAP analyzer

Java CRAP metrics use [antonillos/crap4java](https://github.com/antonillos/crap4java).
Java build and coverage steps run through
[antonillos/makevn](https://github.com/antonillos/makevn) from the repository root.
Verify pins crap4java release **v0.1.0** and checks the JAR's SHA-256 before use. The Java
wrapper supplies JaCoCo coverage and collects JSON in report-only mode; the
combined CRAP wrapper enforces the existing warning-count gate. Code review
reads the generated CI artifacts rather than rerunning the analyzer.

## Commit policy

The independent **Commit Policy** workflow runs on PRs to `develop` and `main`,
without path filters or a dependency on Verify. Its **Conventional Commits and
signatures** check validates every PR commit's header and body separator, and
requires GitHub to verify its SSH signature. Custom types, optional scopes and
breaking-change markers are supported; bot and merge commits are not exempt.

The check reads GitHub commit metadata only: it does not check out PR code,
execute tests or CRAP, or use signing secrets. Logs contain short commit SHAs
and fixed failure categories, not messages or author details. API failures,
unverified signatures, stale revisions and incomplete commit lists fail the
check. PRs above the API's 250-commit limit must be split rather than partially
validated. It does not check a future merge commit or replace privacy review.

Workflow checks and required-check configuration are separate. This change does
not modify repository protections or the existing Verify/CRAP gates.

References: [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/),
[GitHub signature verification](https://docs.github.com/en/rest/git/commits#get-a-commit-object).

## Publication and manual runs

This optimization applies to the PR path classifier. Pushes and ordinary manual
Verify dispatches retain their existing full profile. The `badge_only` refresh
still rebuilds the website and CRAP badge and publishes them together after a
merge to `develop`; it does not repeat every product suite.

`/merge` continues to require a successful Verify run for the exact PR head.
It can reuse that successful PR run; if a manual fallback is needed, the
ordinary full-dispatch behavior remains unchanged. Do not bypass branch
protection to speed up an individual website PR.

Regression checks:

```bash
node --test .github/scripts/ci-path-policy.test.js
python3 -m unittest discover -s tools/ci -p 'test_site_validation.py'
```
