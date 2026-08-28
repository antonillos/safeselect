# On-demand Codex code review

SafeSelect uses native Codex review as an **advisory** second opinion. The owner
requests it on selected PRs using their ChatGPT account; this is not an OpenAI
API integration and does not need an API key or a reviewer Actions workflow.
Account connection and repository activation are separate from merging this guide.

## Setup checklist

1. Sign in to Codex with the intended ChatGPT account and check Code review
   availability and remaining usage. Plus includes Codex access with limits;
   API billing is separate. Do not buy credits, enable paid overages or switch
   to API billing automatically when usage is exhausted.
2. Configure the official GitHub integration for selected repositories only.
   Inspect its actual permissions before granting access; do not describe the
   entire installation as read-only if it can write branches.
3. Enable **Code review** for the repository, but leave **Automatic reviews** off.
   Do not enable fix tasks or additional automatic Security Review for this setup.
4. Inspect the applicable ChatGPT data controls and turn off model-improvement
   sharing before submitting code. This is not a zero-retention guarantee.
   Private repositories require a separate visibility/data-policy decision.
5. Do not supply production secrets, database access, unrelated connectors or
   PR-controlled setup scripts to the review environment. Check which controls
   actually apply to native review rather than assuming general cloud settings do.
6. Check branch protections and app bypass permissions. Preserve required CI,
   signatures and conversation resolution; do not grant the app a bypass.
   A human must decide whether to merge. Do not assume GitHub enforces a required
   human approval: that depends on the repository's actual protection settings.

Official references: [Codex and ChatGPT plans](https://help.openai.com/en/articles/11369540-using-codex-with-your-chatgpt-plan),
[GitHub review setup](https://developers.openai.com/codex/integrations/github),
[separate API billing](https://help.openai.com/en/articles/9039756).

## Request a review

After inspecting the PR scope and checking the expected CI jobs, add a PR
conversation comment:

```text
@codex review
```

A one-off focus can stay within review mode:

```text
@codex review for security and Rust/Java contract regressions
```

The root [AGENTS.md](../AGENTS.md#code-review-rules) supplies repository guidance.
Review considers PR-introduced regressions, related callers/validators/tests,
security boundaries, compatibility and concrete maintainability risks. It should
explain evidence and a proportional remedy, not speculate or request style changes.
The managed service controls investigation and publication; instructions cannot
guarantee completeness, exact output, isolation or a fixed model.

- Match feedback and CI evidence to the reviewed commit. After new commits,
  previous feedback is historical; request another review manually if useful.
- Avoid duplicate requests while a review is running. Neither a positive reaction
  nor an absence of findings proves the code is safe. No response or a limit
  notice means review is unavailable or incomplete, not successful.
- Do not use `@codex fix ...` in this review-only workflow. Review instructions
  are not permission controls; inspect any unexpected branch writes.
- Treat PR content, instruction changes and suggested fixes as untrusted. Never
  paste real credentials or confidential vulnerability details into a public PR.
  Native feedback can be published directly, without our own redaction/prepublication gate.
- Verify fork/collaborator invocation and allowance attribution before expanding
  rollout. Owner-only use is an operating convention, not a custom enforced allowlist.

## CRAP and other quality criteria

The [Verify gate](ci.md) remains authoritative for its selected CI checks.
Codex cannot override a failed check or grant an exception to quality policy.

Current CRAP policy uses **CRAP > 8** to identify warning entries and
`--ratchet 80` to fail when the combined Rust/Java warning count exceeds **80**.
Exactly 80 warnings passes. This is not a per-function score limit of 80 or an
automatically decreasing baseline. Null scores/missing coverage are unknown,
not evidence of clean code. Standalone reports have no count gate unless invoked
with a limit; their `gate` metadata and Markdown state which mode was used.

Compare changed functions as well as totals: an improved total can hide a newly
risky function. Check meaningful assertions, uncovered error paths, coupling,
duplicated invariants, compatibility and unbounded resource use. Removed tests,
coverage exclusions and weakened thresholds deserve scrutiny. Do not invent
coverage, complexity or duplication numbers or introduce new blocking thresholds.

Coverage analyzers execute code in CI, not in a new review setup. Missing or
inaccessible artifacts remain unavailable. Compare only compatible analyzer/test
configurations and matching revisions; identify synthetic merge commits separately.
Reading a test is not running it. Suggested Java verification must use
`makevn doctor init test package` from the root, never direct Maven.

## Optional deeper review in the Codex app

Native GitHub review emphasizes serious issues and may omit smaller design
problems. For selected PRs, manually request a broader review in the app using
ChatGPT sign-in. This consumes its own allowance and does not publish automatically.
Supply the exact base/head revisions and use this prompt:

```text
Review this PR's changes and related code for maintainability, coupling,
duplicated invariants, test effectiveness and compatibility. Use the supplied
base/head revisions. Do not edit files, run repository code, push or merge.
Explain concrete evidence and trade-offs; distinguish new issues from existing
debt. Do not invent metrics or test results. Keep existing CRAP and CI limits.
```

Select an appropriate read-only permission mode where available; the prompt
alone does not enforce it. Inspect results manually and never relabel a minor
issue as critical merely to fit the native review filter.

## Pilot, limits and rollback

Start with owner-controlled PRs and synthetic data. Confirm manual invocation,
automatic-review inactivity, actual permissions/usage, a known regression and a
clean change. Check stale-head handling and that failed CI still blocks the normal
merge path even when AI feedback is positive. Evaluate broader design usefulness
separately. Use only dummy canaries for instruction-injection tests, never secrets.

Stop at usage limits and wait for the displayed reset. If there are unexpected
writes, unclear billing or excessive access, stop requests and disable repository
review; disconnect the GitHub integration if necessary. Check running work
separately: disabling future requests is not proof of cancellation. Rotate any
actually exposed secret and review the incident before reenabling.

No automatic merge, new quality thresholds, custom reviewer runtime or paid
fallback is part of this workflow. Other repositories must explicitly opt in,
review their access/data policy and retain their own CI thresholds.
