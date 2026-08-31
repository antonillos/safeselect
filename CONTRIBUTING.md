# Contributing to SafeSelect

## Before You Start

- Check existing issues and PRs before starting work.
- Open an issue first for significant changes so we can discuss the approach.
- Follow the [Development Principles](#development-principles).

## Development Principles

Apply these principles, adapted from
[antonillos/andrej-karpathy-skills](https://github.com/antonillos/andrej-karpathy-skills/blob/main/CLAUDE.md):

- **Think Before Coding**: Inspect relevant code, make assumptions explicit and
  explain meaningful trade-offs. Resolve ambiguities that affect correctness,
  scope or safety before editing instead of silently choosing an interpretation.
- **Simplicity First**: Implement only the requested behavior with the smallest
  clear solution. Avoid speculative features, unnecessary dependencies and
  abstractions without a current need. Simplicity must not weaken security checks.
- **Surgical Changes**: Keep each edit tied to the task and follow local style.
  Preserve unrelated code and user changes. Remove only dead code introduced by
  your edits; report unrelated cleanup opportunities without acting on them.
- **Goal-Driven Execution**: Define observable acceptance criteria before coding.
  For bugs, add a reproducing test first where feasible; for refactors, verify
  behavior before and after. Map multi-step work to checks and report actual
  results and remaining gaps, not assumed success.

Scale planning and verification to the change. These principles do not override
review-only restrictions, existing CI ownership, privacy rules or fail-closed
security. Review consumes existing CI evidence rather than executing CRAP again.

## Development Setup

Use [antonillos/makevn](https://github.com/antonillos/makevn) for Java build and
test tasks. Run the following from the repository root; do not invoke Maven
directly from the sidecar directory.

```bash
# Initialize, test, and package the Java sidecar
makevn doctor init test package
cp sidecar/target/safeselect-sidecar-*.jar sidecar/target/safeselect-sidecar.jar

# Build Rust binary with the packaged sidecar
cargo build

# Run tests
cargo test

# Run linter
cargo clippy
```

## Pull Request Requirements

- Describe what and why in the PR description.
- Include verification steps.
- Update documentation for user-facing changes.
- Keep backward compatibility unless explicitly breaking.
- Before merging, verify the required checks for the current PR revision.
  Labels and positive AI feedback do not replace those checks or authorize a merge.

## Code Style

- Rust: follow existing patterns and `cargo fmt`.
- Java: keep the sidecar minimal — no frameworks beyond Jackson.
- No secrets, credentials, local paths, or generated artifacts in commits.

## Commit Messages

Every commit must follow Conventional Commits and carry a verifiable PGP or SSH
signature. This applies to every commit in a PR, not only its title or the final
squash commit. The only Commit Policy exception is a GitHub-generated merge
identified by GitHub's `web-flow` committer association and the standard
pull-request merge header; locally-created merges remain subject to signature
validation. A `Signed-off-by` trailer does not replace a cryptographic signature.
The separate [Commit Policy workflow](docs/ci.md#commit-policy) checks message
format and GitHub signature verification; it does not establish that content
is free of private information.

Commit messages, committed files and metadata must not contain private, personal
or confidential information. Use a public contributor handle and a GitHub noreply
address instead of personal contact details. Inspect staged changes and the commit
message before committing; never include credentials, private logs or real user data.

Examples:

```
feat: add new feature
fix: correct a bug
docs: update documentation
security: address a vulnerability
```

## Testing

- Unit tests go next to the module they test (`#[cfg(test)] mod tests`).
- Integration tests go in `tests/`.
- Security-related changes must include tests for both pass and reject cases.
- For Rust changes, run `cargo fmt --check`, `cargo test` and `cargo clippy`.
- For Java changes, follow the root-level [build sequence](#development-setup),
  including the Rust rebuild when the embedded sidecar changes.
- For documentation-only changes, run `python3 tools/ci/validate_docs.py` and
  `git diff --check`; do not rebuild the product solely to validate prose.
- Run targeted tests for changed tooling. Report checks actually executed and
  explain unavailable validation. These local checks do not replace required CI.
- For review-only tasks, use [existing CI evidence](docs/code-review.md#quality-criteria)
  instead of treating these development commands as an instruction to run builds.

## Security

Do not report suspected vulnerabilities in public issues. Follow the instructions in [SECURITY.md](SECURITY.md).
