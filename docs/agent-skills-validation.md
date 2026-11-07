# Agent skill validation

Use isolated homes and application repositories. Keep private credentials,
configuration, model traces and database content out of committed evidence.
Never modify real client permissions or trust merely to force a test to pass.

## Four separate checks

1. Distribution: compare all four SKILL.md files and references with the embedded
   bundle. Check metadata and relative links after installation.
2. Discovery: register in the requested scope, inspect `agent status`, then use
   the installed client's catalog API to verify all four IDs and descriptions.
3. Explicit loading: ask the client to load each named skill without executing
   its workflow. Verify skill invocation or file-read events, not just its answer.
4. Implicit selection: use fresh sessions and requests below without naming a
   skill. Inspect actual skill loads and the proposed workflow decisions.

Use official documentation and help for the installed client version. Codex
app-server exposes `skills/list`; OpenCode V2 exposes `skill.list` on a persistent
server. Initialize OpenCode plugins and agents first so discovery has completed.
Catalog success does not demonstrate explicit loading or implicit selection.
Model checks require an authenticated provider; record unavailable or timed-out
runs as unverified rather than inferring success from files on disk.

## Representative requests

Use hypothetical fixtures and ask for next steps only; do not connect to real
databases, write config or execute queries during these model checks.

| Request | Expected selection and behavior |
| --- | --- |
| Set up database access for this application; two environments exist | base + setup; resolve environment, protect secrets, no guessed registration |
| What tables are available? | base + SQL; database_info/list_tables, stop without row reads |
| Show five recent orders; schema is unfamiliar | base + SQL; exact list_tables/describe_table before a bounded SELECT |
| The grouped query timed out | base + SQL; preserve/narrow predicates, non-executing explain; no unchanged retry or automatic limit increase |
| Explore document fields in this collection | base + MongoDB; exact database/collection discovery and sampled schema, no invented fields |
| A flattened filter was rejected | MongoDB; resend complete nested/encoded filter, never replace with an empty filter |
| Serious security rejection stopped the server | base; stop and report, host restart after correction; no reconnect on a dead session |
| My external bastion/VPN/tunnel is down and I must restore it myself | base + setup; stop, ask for manual restoration, no check/reconnect/shell loop; wait for user confirmation |
| The bastion works but the database session is disconnected | base + setup; one direct MCP reconnect before check, retry only after success; stop on failure |

## Implementation evidence (2026-10-23)

- SafeSelect 0.8.0 embeds all four skills and references. Automated tests compare
  source and embedded hashes, check metadata, cover both clients/scopes, repeated
  setup, MCP-only migration, shared resources, unchanged-owned updates,
  conflicts, unsafe permissions, symlinks, lock contention and operation rollback.
  Final local checks passed: 526 Rust tests, `cargo fmt --check`, `cargo clippy`,
  `python3 tools/ci/validate_docs.py` and `git diff --check`. The existing build
  script emits its informational embedded-JAR warning; Java was not rebuilt.
- Codex CLI 0.152.0: isolated HOME/CODEX_HOME, app-server `initialize` followed by
  `skills/list` with `forceReload: true`. All four skills were discovered and
  enabled, with no skill errors, separately in user and repository scopes.
- OpenCode 2.0.32: isolated HOME/XDG directories, authenticated loopback server,
  `plugin.list`, `agent.list`, then `skill.list`. All four IDs and Markdown bodies
  were discovered separately in global and project scopes.
- OpenCode explicit loading: `experimental.session.skill` activated each of the
  four IDs with `resume: false`. `session.context` contained four `type: skill`
  messages with the corresponding bodies. No model or database workflow ran.
- Codex explicit loading with a model and implicit-selection evaluations in both
  clients remain unverified. The isolated Codex profile reported `Not logged in`.
  Authenticated model evaluations were unavailable in the isolated profiles.
  A future authenticated evaluation must record actual read/tool events and
  outcomes for the scenarios above before claiming model behavior.

These are local observations, not CI evidence, and do not guarantee automatic
selection for arbitrary models and requests. Source checks and catalog responses
are never substituted for missing model evaluations.
