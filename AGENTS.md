# SafeSelect — AI Agent Instructions

This file provides instructions for AI agents working on the SafeSelect repository.

## Overview

SafeSelect is a Rust CLI + Java sidecar that implements a secure MCP (Model Context Protocol)
SQL proxy for AI agents. Fail-closed security model: any incident terminates the process.

## Project Structure

- `src/` — Rust source code (CLI, MCP server, config, security, audit)
- `sidecar/` — Java sidecar (JDBC proxy via stdin/stdout JSON-lines)
- `skills/` — OpenCode skill manifest
- `docs/` — Documentation
- `packaging/homebrew/` — Homebrew formula (published to antonillos/homebrew-tap)

## Key Architecture Decisions

- **Communication**: Rust ↔ Java via stdin/stdout JSON-lines (no network, no sockets)
- **Config**: TOML, hierarchical by project/environment, macOS Keychain for secrets
- **Security**: fail-closed (`std::process::exit(1)`), read-only enforcement, SHA-256 drivers
- **Distribution**: single binary with embedded sidecar JAR

## Development Workflow

1. `cargo build` — builds the Rust binary with embedded sidecar
2. `cargo test` — runs Rust unit tests
3. `cd sidecar && mvn package` — rebuilds the Java sidecar
4. After rebuilding sidecar, copy JAR to expected name and rebuild Rust

## Commands

- `cargo check` — quick validation
- `cargo clippy` — lint
- `cargo fmt` — format
- `cargo test` — run tests

## Preferences

- Use `fff` MCP tools for file and code search.
- Prefer `rtk` wrappers for shell commands when available.
- Sign commits with SSH.
- Follow conventional commits.
