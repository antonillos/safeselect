# Contributing to SafeSelect

## Before You Start

- Check existing issues and PRs before starting work.
- Open an issue first for significant changes so we can discuss the approach.

## Development Setup

```bash
# Build Rust binary
cargo build

# Build Java sidecar
cd sidecar && mvn package && cp target/safeselect-sidecar-*.jar target/safeselect-sidecar.jar

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
- After adding the `safe-to-merge` label, wait for the security check to pass before merging.

## Code Style

- Rust: follow existing patterns and `cargo fmt`.
- Java: keep the sidecar minimal — no frameworks beyond Jackson.
- No secrets, credentials, local paths, or generated artifacts in commits.

## Commit Messages

Every commit must follow Conventional Commits and carry a verifiable SSH
signature. This applies to every commit in a PR, not only its title or the final
squash commit. A `Signed-off-by` trailer does not replace a cryptographic signature.

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
- Run `cargo test` and `cargo clippy` before opening a PR.

## Security

Do not report suspected vulnerabilities in public issues. Follow the instructions in [SECURITY.md](SECURITY.md).
