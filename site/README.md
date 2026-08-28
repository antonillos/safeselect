# SafeSelect marketing site

Canonical public home: <https://antonillos.github.io/safeselect/>.

This site has no database, visitor accounts, query telemetry or external fonts.
The public GitHub Pages output is static HTML/CSS; JavaScript is not required.
Sites provides an isolated review deployment, not a second SEO identity.

## Local development

Use Node 24 and the committed npm lockfile:

```bash
npm ci --ignore-scripts
npm run dev
```

`predev` and `prebuild` prepare the three reviewed Markdown documents from
`../docs/` and copy the existing onboarding GIF. Those generated copies are
ignored. The social card is `public/og.png`.

## Validate and build

### Brand assets

`public/icon.svg` is the editable source. Its background, tier gaps and lens
interior are transparent; the lens masks the underlying write-access tier.
The cylinder tiers have a subtle 1.5-unit white outline on the 512-unit canvas.
`public/icon-dark.svg` is generated with a lighter palette for dark surfaces.
The README selects the matching SVG using `<picture>`; the light website uses
the original. No opaque square is embedded in either variant or the PNGs.

After editing the source, run `npm run icons:export`. Validate the derivatives
without writing with `npm run icons:export -- --check`. Equal outputs are not
rewritten. Exports use sharp from the committed npm lockfile; different renderer
versions may produce different PNG bytes. Keep the SVG, dark variant and the
32/180/512px PNG derivatives together in the PR. The existing wide Open Graph
card remains separate from this square icon.

`npm run test:icons` checks transparency (including the lens), palettes, size
budgets and that repeated generation/checking leaves current assets unchanged.

```bash
npm run build:pages
npm run typecheck
npm run lint
python3 ../tools/ci/validate_site.py
npm run build
```

`build:pages` renders the same page components without hydration into `out/`,
using the `/safeselect` base path. It emits a sitemap and per-page canonical,
Open Graph and X metadata. Detail pages do not inherit the homepage image.
`build` produces the separate Sites Worker deployment.

Verify uploads the static output and the current badge as separate intermediate
artifacts, then combines them into **one** GitHub Pages artifact. Never add a
second independent Pages deploy that could replace either part.

## Editorial source and review snapshots

Edit the comparison, guide and article in `../docs/`, not their generated JSON.
The build accepts only those three fixed source files; it does not render
visitor-supplied Markdown. Other repository documentation links point to GitHub.

When preparing an isolated Sites source repository, include the exact reviewed
Markdown under `source-docs/docs/` (retaining its subdirectories) and the
onboarding GIF at `source-docs/docs/recordings/onboarding-full-local.gif`.
The preparation script uses this fallback only when the parent repository docs
are unavailable. Keep `.openai/hosting.json` with the same project ID and do not
push the unrelated Rust repository to the Sites source remote.

The deployed private review is not indexable by the public. Follow the
[publication checklist](../docs/positioning.md) after merging to publish and
measure the canonical public site.
