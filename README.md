# SafeSelect

**Fail-closed, read-only SQL and NoSQL database access for AI agents over MCP.**

[![CI](https://github.com/antonillos/safeselect/actions/workflows/verify.yml/badge.svg)](https://github.com/antonillos/safeselect/actions/workflows/verify.yml)
[![Security](https://img.shields.io/badge/Security-fail--closed-success?logo=trustpilot&logoColor=white)]()
[![Rust](https://img.shields.io/badge/Rust-1.81%2B-dea584?logo=rust&logoColor=white)]()
[![Java](https://img.shields.io/badge/Java-17%2B-5382a1?logo=openjdk&logoColor=white)]()
[![MCP](https://img.shields.io/badge/MCP-0.1.0-7b68ee)]()
[![Homebrew](https://img.shields.io/badge/Homebrew-tap-FBB040?logo=homebrew&logoColor=white)](https://github.com/antonillos/homebrew-tap)
[![asdf](https://img.shields.io/badge/asdf-plugin-8A2BE2)](https://github.com/antonillos/asdf-safeselect)
[![License](https://img.shields.io/badge/License-MIT-yellow)](LICENSE)

SafeSelect gives coding agents a constrained database tool for SQL and NoSQL systems: discover structure, inspect data, run read-only operations, diagnose connectivity, and recover stale connections without ever getting write access.

Current backend support: PostgreSQL and MongoDB.

## Architecture

<p align="center">
  <img src="docs/safeselect-architecture.svg" alt="SafeSelect Architecture" width="800">
</p>

The agent talks to SafeSelect through MCP stdio. SafeSelect enforces policy in Rust, stores secrets outside project files, and reaches databases through an embedded Java sidecar: JDBC for SQL backends and the MongoDB driver for MongoDB. The Rust to Java channel is JSON-lines over stdin/stdout: no sockets, no open ports.

## Quick Start

```bash
brew install antonillos/tap/safeselect

# Import a project database
safeselect import-dbeaver ~/Downloads/dbeaver-export.zip
# or:
# safeselect import-compose
# safeselect import-compass --path "$HOME/.config/MongoDB Compass"

# Verify the environment
safeselect check --environment testing

# Install the MCP entry for your agent
safeselect agent install opencode --environment testing
```

The generated MCP name defaults to `safeselect-<project>-<environment>`.

## Agent Workflow

Agents should use SafeSelect in this order:

1. `database_info`
2. `list_tables` for SQL, or `list_databases` / `list_collections` for NoSQL
3. `select`, `explain`, or `find_documents`
4. `check`, `connect`, or `reconnect` when connectivity is stale

Query responses include `row_count`, `byte_count`, `elapsed_ms`, and a human-readable `elapsed` value so agents can reason about result size and latency.

## Security Model

- **Fail closed**: security violations terminate the MCP process.
- **Read only**: SQL allows `SELECT`, `EXPLAIN`, and `WITH`; NoSQL backends allow discovery and read-only document reads.
- **Scoped access**: schemas, relations, databases, and collections can be allowed or denied.
- **Hard limits**: row count, result bytes, and timeouts are enforced.
- **Secret isolation**: passwords live in macOS Keychain or environment variables, never in project config.
- **Driver verification**: JDBC drivers are checked by SHA-256 before use.
- **Audit trail**: query text is hashed before being recorded.

## MCP Tools

| Area | Tools |
|---|---|
| SQL | `list_tables`, `select`, `explain` |
| NoSQL | `list_databases`, `list_collections`, `find_documents` |
| Connection | `database_info`, `check`, `connect`, `disconnect`, `reconnect` |
| Config | `config_validate`, `config_show`, `config_set_password`, `config_set_ssh_password`, `config_reset` |
| Setup | `import_compose`, `driver_list`, `driver_add`, `driver_download`, `agent_detect`, `agent_install`, `agent_status`, `agent_uninstall` |

When no `.safeselect/` directory exists, `safeselect serve --environment <env>` enters setup mode automatically and exposes only the setup-safe tools.

## CLI Essentials

| Command | Purpose |
|---|---|
| `safeselect serve --environment <env>` | Start the MCP server |
| `safeselect check --environment <env>` | Verify config, secrets, tunnels, sidecar, and backend connectivity |
| `safeselect doctor --environment <env>` | Print deeper diagnostics with stable codes |
| `safeselect import-dbeaver <zip>` | Import DBeaver connections |
| `safeselect import-compose [--path <path>]` | Import from docker-compose |
| `safeselect import-compass [--path <path>]` | Import MongoDB Compass connections |
| `safeselect agent install <client> --environment <env>` | Install an MCP entry |
| `safeselect config set-password --environment <env>` | Store the database password |
| `safeselect config set-ssh-password --environment <env>` | Store the SSH password |

Use `safeselect --help` or a command-specific `--help` for the full CLI.

## Configuration

Global state lives in `~/.config/safeselect/` by default. Project policy lives in `.safeselect/` at the repository root:

```text
<repo-root>/
└── .safeselect/
    ├── project.toml
    └── environments/
        └── <env>.toml
```

SafeSelect walks upward from the current directory to find `.safeselect/`. Use `--project <path>` when an agent or script should target a specific repository.

## Supported Agents

- OpenCode: install supported
- GitHub Copilot, Cursor, Windsurf, Claude Code, Codex, Gemini CLI: detection supported

## Build From Source

```bash
./install.sh
safeselect --version
```

Requirements: Rust 1.81+, Java 17+, Maven 3.8+. `sshpass` is optional for password-based SSH tunnels.

## Documentation

- [Installation guide](docs/install.md)
- [AI agent integration](docs/agents.md)
- [Security model](docs/security.md)
- [Distribution](docs/distribution.md)
- [Changelog](CHANGELOG.md)

Release notes are generated from `CHANGELOG.md`.

## License

MIT - see [LICENSE](LICENSE).
