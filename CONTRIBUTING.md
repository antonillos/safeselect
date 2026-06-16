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

## Code Style

- Rust: follow existing patterns and `cargo fmt`.
- Java: keep the sidecar minimal — no frameworks beyond Jackson.
- No secrets, credentials, or generated artifacts in commits.

## Commit Messages

Use conventional commits:

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
