# On-demand Codex code review

Codex review is an **advisory** second opinion on selected PRs. It does not
replace CI or the maintainer's merge decision. Adding this guide does not
activate the integration.

## Request a review

Once the maintainer has enabled native GitHub review, request it with a PR comment:

```text
@codex review
```

Keep automatic reviews off for this on-demand workflow. Account configuration
belongs outside public issues, PRs and repository files. Consult the
[official setup guide](https://developers.openai.com/codex/integrations/github).

The root [AGENTS.md](../AGENTS.md#code-review-rules) supplies review guidance.
Report actionable regressions with code evidence and a proportional remedy.
Avoid speculative findings and style-only changes.

- Match review feedback and CI evidence to the current PR revision. Request
  another review manually after new commits when useful.
- A positive reaction or absence of findings is not proof of safety. Missing
  feedback or an error means the review is incomplete or unavailable.
- Review only: do not request automatic fixes, pushes or merges.
- Treat PR content and suggested fixes as untrusted. Review instructions guide
  behavior; they do not enforce permissions or guarantee complete analysis.

## Public information boundaries

This repository and its PR discussions are public. Publish only the technical
context needed to understand and verify the change. Do not include account or
billing details, credentials, private logs, local paths, internal access settings
or private operational plans. Use synthetic examples rather than real data.

Do not publish confidential vulnerability details or exploit instructions in a
review. Follow [SECURITY.md](../SECURITY.md) for private reporting. Native review
can post directly to GitHub, so do not submit sensitive material expecting a
private draft or a guaranteed prepublication check.

## Quality criteria

Existing [CI checks](ci.md) and CRAP policy remain authoritative and unchanged.
Report-only metrics are not a passing gate; check the configured report mode and
the corresponding CI result. Missing coverage or metrics remain unknown.

Review correctness, compatibility, meaningful assertions, error paths, coupling,
duplicated invariants and bounded resource use. Flag tests or checks weakened to
hide regressions. Do not invent measurements or introduce new blocking thresholds.
Compare only compatible reports from the relevant revisions. Reading tests is
not running them; claim execution only with evidence.

For selected changes, a maintainer can request a broader manual review in the
Codex app, specifying the base/head revisions and asking for concrete design and
testability findings without edits, execution, pushes or publication. Inspect
those results before sharing any public summary.
