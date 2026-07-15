# SafeSelect

**Fail-closed, read-only SQL and NoSQL database access for AI agents over MCP.**

[![CI](https://github.com/antonillos/safeselect/actions/workflows/verify.yml/badge.svg)](https://github.com/antonillos/safeselect/actions/workflows/verify.yml)
[![Security](https://img.shields.io/badge/Security-fail--closed-success?logo=trustpilot&logoColor=white)]()
[![Rust](https://img.shields.io/badge/Rust-1.81%2B-dea584?logo=rust&logoColor=white)]()
[![Java](https://img.shields.io/badge/Java-17%2B-5382a1?logo=openjdk&logoColor=white)]()
[![MCP](https://img.shields.io/badge/MCP-stdio%20tools-7b68ee)]()
[![Homebrew](https://img.shields.io/badge/Homebrew-tap-FBB040?logo=homebrew&logoColor=white)](https://github.com/antonillos/homebrew-tap)
[![asdf](https://img.shields.io/badge/asdf-plugin-8A2BE2)](https://github.com/antonillos/asdf-safeselect)
[![License](https://img.shields.io/badge/License-MIT-yellow)](LICENSE)

SafeSelect gives coding agents a constrained database tool for SQL and NoSQL systems: discover structure, inspect data, run read-only operations, diagnose connectivity, and recover stale connections without ever getting write access.

Most database MCP servers make it easy to connect an agent to a database. SafeSelect is built for the harder problem: letting an agent inspect production-shaped data without turning the database into an unrestricted tool surface.

> [!NOTE]
> SafeSelect is a safety boundary for agent access, not a replacement for database permissions. Use least-privilege database users when you can; SafeSelect still constrains overpowered credentials when agents connect through it.

Current backend support: PostgreSQL and MongoDB.

## Why SafeSelect?

SafeSelect is intentionally narrower than general-purpose database MCP servers. It is not a tool builder, SQL workbench, or remote database gateway. It is a local safety boundary for agents that need database visibility, not database power.

| SafeSelect prioritizes | What this means |
|---|---|
| Local stdio transport | No network listener or open MCP port |
| Read-only tools | Agents do not receive write-capable database tools |
| Credential-independent safety | Even DBA credentials are constrained to SafeSelect's read-only tool surface |
| Fail-closed enforcement | Policy violations terminate the process |
| Secret isolation | Passwords stay in Keychain or environment variables |
| Project-scoped policy | Each repository defines its own allowed data surface |
| Embedded sidecar | One installed binary reaches JDBC and MongoDB drivers behind Rust policy |

## What Makes It Different?

| General database MCP servers | SafeSelect |
|---|---|
| Often expose configurable tools | Exposes a fixed, read-only tool surface |
| May support remote HTTP transports | Uses local MCP stdio by default |
| Usually optimize for broad backend coverage | Optimizes for enforceable policy and agent safety |
| Often rely on least-privilege database users | Enforces read-only behavior even when credentials are overpowered |
| Often keep connection setup separate | Imports from DBeaver, Docker Compose, and MongoDB Compass |
| May log queries for debugging | Hashes query text before audit logging |
| Treat security failures as recoverable errors | Fails closed and terminates the MCP process |

The product promise is simple: **agents can look, but they cannot mutate**. Even if the configured database user is a DBA, the agent still only receives SafeSelect's constrained read-only operations.

> [!TIP]
> This is useful when teams already have DBeaver, Docker Compose, or MongoDB Compass connections and need to expose them to agents without redesigning database users first.

## Backend Support

| Backend | Status | Tools |
|---|---|---|
| PostgreSQL | Supported | `list_tables`, `select`, `explain` |
| MongoDB | Supported | Discovery, find, aggregation, distinct/count, explain, profiling, schema inference, and anonymized fixtures |

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

The generated MCP entry is a stdio server scoped to one project and environment:

```json
{
  "mcpServers": {
    "safeselect-myapp-testing": {
      "command": "safeselect",
      "args": ["serve", "--project", "/path/to/myapp", "--environment", "testing"]
    }
  }
}
```

See [AI agent integration](docs/agents.md) for client-specific setup and manual configuration.

## Agent Workflow

Agents should use SafeSelect in this order:

1. `database_info`
2. `list_tables` for SQL, or `list_databases` / `list_collections` for NoSQL
3. `select` / `explain`, or the bounded MongoDB read tool that matches the task
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
| MongoDB reads | `list_databases`, `list_collections`, `find_documents`, `aggregate_documents`, `distinct_documents`, `count_documents`, `explain_documents` |
| MongoDB analysis | `profile_document_field`, `discover_document_schema`, `generate_document_fixture` |
| Connection | `database_info`, `check`, `connect`, `disconnect`, `reconnect` |
| Config | `config_validate`, `config_show`, `config_set_password`, `config_rename_environment`, `config_delete_environment`, `config_reset` |
| Setup | `import_compose`, `driver_list`, `driver_add`, `driver_download`, `agent_detect`, `agent_install`, `agent_status`, `agent_uninstall` |

When no `.safeselect/` directory exists, `safeselect serve --environment <env>` enters setup mode automatically and exposes only the setup-safe tools.

> [!IMPORTANT]
> Setup mode does not expose query tools. Agents can help import and validate configuration before any database inspection tools become available.

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
| `safeselect uninstall` | Remove installed binaries, global state, audit data, and Keychain entries |

Use `safeselect --help` or a command-specific `--help` for the full CLI.

Interactive OpenCode installation lets you choose the project-local JSON config,
create a separate `.opencode/opencode.jsonc` when appropriate, or use the global config.
Uninstall checks both release-installer and Cargo binary locations.

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
- [Security policy](SECURITY.md)
- [Distribution](docs/distribution.md)
- [Changelog](CHANGELOG.md)

Release notes are generated from `CHANGELOG.md`.

## License

MIT - see [LICENSE](LICENSE).
