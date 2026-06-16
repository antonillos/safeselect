# SafeSelect

> **MCP SQL Fail-Closed for AI Agents**

[![Rust](https://img.shields.io/badge/Rust-1.81%2B-dea584?logo=rust&logoColor=white)]()
[![MCP](https://img.shields.io/badge/MCP-0.1.0-7b68ee)]()
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-JDBC-336791?logo=postgresql&logoColor=white)]()
[![macOS Keychain](https://img.shields.io/badge/Secrets-macOS%20Keychain-000000?logo=apple&logoColor=white)]()
[![License](https://img.shields.io/badge/License-MIT-yellow)](LICENSE)

SafeSelect is a secure SQL proxy that sits between AI coding agents and your
databases. It implements the **Model Context Protocol (MCP)** to expose
`select`, `list_tables`, and `explain` tools — with a **fail-closed** security
model that terminates the process on any incident.

---

## Security Model

SafeSelect is built with a **security-first** design:

- **Fail-closed**: any security violation kills the MCP process immediately
- **Read-only**: only `SELECT` and `EXPLAIN` queries allowed by default
- **Single statement**: multi-statement SQL is rejected
- **Schema control**: allowed schemas and denied relations enforced
- **SHA-256 drivers**: JDBC driver `.jar` files are checksummed on every use
- **macOS Keychain**: secrets stored securely, never in config files
- **Password isolation**: passwords passed via stdin, never as CLI arguments
- **Audit log**: all queries hashed (SHA-256), never stored in plain text
- **Result limits**: row count and byte size limits enforced

---

## Installation

### Homebrew (macOS)

```bash
brew install antonillos/tap/safeselect
```

### asdf (Linux & macOS)

```bash
asdf plugin add safeselect https://github.com/antonillos/asdf-safeselect.git
asdf install safeselect latest
asdf set -u safeselect latest
asdf reshim safeselect latest
```

### From source

```bash
git clone https://github.com/antonillos/safeselect.git
cd safeselect
cargo build --release
```

Requires: Rust 1.81+, Java 17+, Maven 3.8+

---

## Quick Start

### 1. Configure a project

```bash
mkdir -p ~/.config/safeselect/projects/myapp/environments
```

Create `~/.config/safeselect/projects/myapp/project.toml`:

```toml
version = 1
display_name = "My App"

[security]
read_only = true
allowed_schemas = ["public"]
denied_relations = ["public.users_credentials"]

[limits]
statement_timeout_ms = 5000
max_rows = 500
max_result_bytes = 2_000_000

[audit]
enabled = true
```

Create `~/.config/safeselect/projects/myapp/environments/testing.toml`:

```toml
version = 1

[database]
driver = "postgresql"
url = "jdbc:postgresql://localhost:5432/myapp"
username = "reader"

[database.secret]
source = "macos-keychain"
service = "safeselect"
account = "myapp/testing"

[tls]
mode = "verify-full"
```

### 2. Store the password in Keychain

```bash
security add-generic-password -a "myapp/testing" -s "safeselect" -w "your-password-here"
```

### 3. Register a JDBC driver

```bash
# Download the official PostgreSQL driver
safeselect driver download --vendor postgresql

# Or register a custom corporate driver
safeselect driver add --vendor postgresql --path /path/to/postgresql.jar --class org.postgresql.Driver
```

### 4. Test the connection

```bash
safeselect check --project myapp --environment testing
```

### 5. Install in your AI agent

```bash
# OpenCode
safeselect agent install opencode --project myapp --environment testing --name myapp-testing

# Other agents
safeselect agent detect
safeselect agent install cursor --project myapp --environment testing --name myapp-testing
```

---

## MCP Tools

Once `safeselect serve` is running, AI agents can call these tools:

| Tool | Description | Arguments |
|---|---|---|
| `select` | Execute a SELECT query | `sql` (required): the query |
| `list_tables` | List database tables | `schema` (optional): filter by schema |
| `explain` | Show execution plan | `sql` (required): the query (not executed) |

---

## CLI Reference

| Command | Description |
|---|---|
| `serve --project <p> --environment <e>` | Start the MCP server for a project/environment |
| `config validate [--project <p>] [--environment <e>]` | Validate configuration |
| `config show --project <p> --environment <e>` | Show resolved configuration (secrets redacted) |
| `driver add --vendor <v> --path <jar> --class <c>` | Register a JDBC driver |
| `driver list` | List registered drivers |
| `driver download --vendor postgresql` | Download official PostgreSQL driver |
| `agent detect` | Detect installed MCP clients |
| `agent install <client> --project <p> --environment <e> --name <n>` | Install MCP entry for a client |
| `agent uninstall <client> --name <n>` | Remove a SafeSelect MCP entry |
| `agent status` | Show installation status |
| `check --project <p> --environment <e>` | Test connectivity end-to-end |
| `import dbeaver <path-to-zip>` | Import configuration from DBeaver export |

---

## Configuration Layout

```
~/.config/safeselect/
├── drivers/
│   └── postgresql.toml          # JDBC driver registration
├── projects/
│   └── <project-name>/
│       ├── project.toml          # Security policy + limits + audit
│       └── environments/
│           ├── testing.toml      # Connection details + secrets
│           ├── staging.toml
│           └── production.toml
```

Each project defines a **maximum policy** that no environment can relax.
Environments can only tighten limits, never expand them.

---

## Architecture

```
┌─────────────┐     stdin/stdout      ┌──────────────┐     JDBC      ┌──────────┐
│  AI Agent   │ ◄──── JSON-RPC ────►  │  safeselect  │ ◄────► ────► │ PostgreSQL│
│ (OpenCode)  │     (MCP protocol)    │  (Rust CLI)  │              │ (or any) │
└─────────────┘                       └──────┬───────┘              └──────────┘
                                             │
                                    stdin/stdout JSON-lines
                                             │
                                      ┌──────▼───────┐
                                      │  sidecar.jar  │
                                      │   (Java)      │
                                      └──────────────┘
```

- Rust process: CLI, MCP server, config, security validation, audit logging
- Java sidecar: JDBC proxy, embedded in the Rust binary, extracted at runtime
- No network between Rust and Java: pure stdin/stdout
- No open ports: MCP is initiated by the agent

---

## Supported AI Agents

- OpenCode
- OpenAI Codex
- Claude Code
- GitHub Copilot (VS Code)
- Cursor
- Windsurf
- Gemini CLI

---

## Documentation

- [Installation guide](docs/install.md)
- [AI agent integration](docs/agents.md)
- [Security model](docs/security.md)
- [Distribution](docs/distribution.md)

---

## License

MIT &ndash; see [LICENSE](LICENSE).
