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
