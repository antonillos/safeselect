# Positioning, distribution and measurement

## Canonical identity

- Name: **SafeSelect MCP**.
- Public home: <https://antonillos.github.io/safeselect/>.
- Category: read-only database MCP for coding agents.
- Short description: **Read-only PostgreSQL & MongoDB access for coding agents.
  Local, fail-closed MCP enforcement with project-scoped policies.**
- Supporting message: debug with database context, without exposing write tools.
- Primary activation path: [DBeaver → Codex](guides/dbeaver-codex.md).
- Evidence: [Security Proof](security-proof.md), [suite](security-test-suite.md),
  [comparison](compare.md), [recordings](../demo/README.md#demo-gallery).

Use the full name to distinguish the project from unrelated SafeSelect and
SAFE-MCP products. Keep PostgreSQL and MongoDB near the first description.
Do not claim zero risk, universal security, comprehensive PII protection or
control over another tool's database access.

## Publication sequence

1. Merge the reviewed positioning PR into `develop`. Verify's single Pages
   publication contains both the website and `crap-badge.json`.
2. Verify the public home, comparison, tutorial, article, social image and
   sitemap. A private Sites review URL is not a substitute for public indexing.
3. Set GitHub About to the short description above and its website to the
   canonical home. Do not point users at a page that has not deployed yet.
4. Review the existing [mcpservers.org listing](https://mcpservers.org/servers/antonillos/safeselect)
   before requesting a refresh. Do not submit a duplicate.
5. Check for existing entries before submitting to Glama or another directory.
   Record the actual submitted URL and approval state, not just an intention.
6. Publish the practical tutorial in one relevant community with clear author
   disclosure. Answer questions before repeating the launch elsewhere.

No product release, tag movement or `develop` → `main` promotion is required
solely for this documentation/website change.

## Ready-to-adapt distribution copy

### Directory summary

SafeSelect MCP gives coding agents read-only PostgreSQL and MongoDB access.
It runs locally over stdio, applies project-scoped policies and result limits,
and terminates on security violations. Import connections from DBeaver, Docker
Compose or MongoDB Compass. Java 17+ required. Use least-privilege database roles.

### Practical tutorial introduction

I maintain SafeSelect MCP. This walkthrough takes an existing DBeaver PostgreSQL
connection into Codex without pasting database credentials into the chat. It
covers local setup, a project-scoped MCP entry and one bounded read. The goal is
database context for debugging, not database administration.

Link to the [published guide](https://antonillos.github.io/safeselect/guides/dbeaver-codex/)
only after deployment. Adapt the wording to community rules; do not claim users,
benchmarks or endorsements that have not been observed.

### Short social post

SafeSelect MCP: read-only PostgreSQL & MongoDB context for coding agents.
Local policies, bounded reads, reproducible security tests.
Start with your DBeaver connection: https://antonillos.github.io/safeselect/

### Technical article

Use [Read-only is not a boolean](read-only-is-not-a-boolean.md). When
cross-posting, set the canonical URL to the original article if the platform
supports it. Link to the comparison and evidence, not just an install command.

## Manual measurement, without query telemetry

Keep private traffic baselines in the ignored local `PLAN.md`, not this public
document. Record dates, observation windows and definitions.

| Stage | Metric | Interpretation |
| --- | --- | --- |
| Discovery | Search Console impressions/clicks, brand vs non-brand | Search visibility, not installations |
| Acquisition | GitHub visitors/clones, release asset downloads | Interest; downloads can include CI, checksums and repeat downloads |
| Activation | External user confirms database identity plus one bounded read | Actual first use, collected voluntarily |
| Retention | A user independently reports later use | Stronger than a star or download |

For a property you control, verify the URL-prefix property in Google Search
Console and submit `https://antonillos.github.io/safeselect/sitemap.xml`.
Use a verification file or tag only from that account—never invent a token.
The repository cannot control the owner site's root `robots.txt` through a
file served beneath `/safeselect/`.

Repeat the five-query Google baseline in 2–4 weeks with the same locale/language
settings. Record the date and first-page observations; don't infer global rank,
search volume or indexing failure from a single search. See
[Google's SEO guidance](https://developers.google.com/search/docs/fundamentals/seo-starter-guide).

Ask early users only for non-sensitive feedback: client/OS, setup step that
failed, approximate time to first read and whether they would use it again.
Never request query contents, result rows, credentials or a connection export.

### Distribution log template

| Date | Channel | Existing listing checked | Submission URL | State | Referrals / confirmed activation |
| --- | --- | --- | --- | --- | --- |
| YYYY-MM-DD | Channel name | Yes / no | Actual URL | Draft / submitted / live | Observed data only |

Creating this kit is not evidence that an external post or directory submission
has been published. Keep those states separate in the local plan.
