# Temporary braces security patch

This private package contains the runtime sources and MIT license from the npm
`braces@3.0.3` distribution. It is installed as `braces` through the root website
package's npm override, including when lifecycle scripts are disabled.

Upstream has no patched release for
[GHSA-vfj7-8cjw-p6xm](https://github.com/advisories/GHSA-vfj7-8cjw-p6xm)
([upstream issue](https://github.com/micromatch/braces/issues/70)). The local
package name and prerelease version identify our patched copy; they do not claim
that upstream 3.0.3 is safe. npm audit does not independently assess this local
code, so the regression tests are required alongside the audit.

Changes from upstream runtime sources:

- `lib/utils.js`: add a fixed nesting limit of 100 with a `SyntaxError` rejection.
- `lib/parse.js`: check the limit before adding brace or parenthesis containers.
  Escaped, quoted and character-class delimiters remain literal.
- `lib/compile.js`, `lib/expand.js`, `lib/stringify.js`: bound recursive container
  traversal, including callers supplying an AST directly.

The package metadata is reduced to the runtime fields and marked private. The
upstream `index.js`, `lib/constants.js` and license are unchanged. Vendored code
is excluded from application ESLint rules; regression tests remain linted.

Run `npm run test:dependencies --prefix site` after `npm ci --prefix site
--ignore-scripts`. Replace this override with a verified upstream fix when one
is available, then remove this directory and update the regression tests.
