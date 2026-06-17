# SafeSelect

**MCP SQL Fail-Closed for AI Agents**

[![CI](https://github.com/antonillos/safeselect/actions/workflows/verify.yml/badge.svg)](https://github.com/antonillos/safeselect/actions/workflows/verify.yml)
[![Security](https://img.shields.io/badge/Security-fail--closed-success?logo=trustpilot&logoColor=white)]()
[![Rust](https://img.shields.io/badge/Rust-1.81%2B-dea584?logo=rust&logoColor=white)]()
[![Java](https://img.shields.io/badge/Java-17%2B-5382a1?logo=openjdk&logoColor=white)]()
[![MCP](https://img.shields.io/badge/MCP-0.1.0-7b68ee)]()
[![Homebrew](https://img.shields.io/badge/Homebrew-tap-FBB040?logo=homebrew&logoColor=white)](https://github.com/antonillos/homebrew-tap)
[![asdf](https://img.shields.io/badge/asdf-plugin-8A2BE2)](https://github.com/antonillos/asdf-safeselect)
[![License](https://img.shields.io/badge/License-MIT-yellow)](LICENSE)

SafeSelect is a secure SQL proxy between AI coding agents and your databases. It implements the **Model Context Protocol (MCP)** with a fail-closed security model — any incident terminates the process.

---

## Architecture

<p align="center">
  <img src="docs/safeselect-architecture.svg" alt="SafeSelect Architecture" width="800">
</p>

- All communication between Rust and Java is JSON-lines over stdin/stdout — no network, no sockets, no open ports
- The Java sidecar is embedded in the Rust binary and extracted at runtime

---

## Quick Start

```bash
# 1. Install
brew install antonillos/tap/safeselect

# 2. Download a JDBC driver
safeselect driver download --vendor postgresql

# 3. Configure project + environment
safeselect import dbeaver export.zip
# Or create manually (see Configuration below)

# 4. Store the password
security add-generic-password -a "myapp/testing" -s "safeselect" -w "your-password"

# 5. Test it
safeselect check --project myapp --environment testing

# 6. Install in OpenCode
safeselect agent install opencode --project myapp --environment testing --name myapp-testing
```

---

## Security Model

- **Fail-closed**: any security violation kills the MCP process immediately
- **Read-only**: only `SELECT`, `EXPLAIN`, and `WITH` queries allowed
- **Single statement**: multi-statement SQL rejected
- **Schema control**: allow/deny specific schemas and relations
- **SHA-256 drivers**: JDBC JAR checksummed on every use
- **macOS Keychain**: secrets never stored in config files
- **Password isolation**: passed via stdin, never as CLI args
- **Result limits**: row count and byte size enforced

---

## CLI Reference

| Command | Description |
|---|---|
| `serve --project <p> --environment <e>` | Start the MCP server |
| `query --project <p> --environment <e> --sql <q>` | Execute SQL directly |
| `check --project <p> --environment <e>` | Test connectivity |
| `config validate [--project <p>] [--environment <e>]` | Validate config |
| `config show --project <p> --environment <e>` | Show resolved config |
| `driver download --vendor postgresql` | Download JDBC driver |
| `driver add --vendor <v> --path <jar> --class <c>` | Register custom driver |
| `driver list` | List registered drivers |
| `agent install <client> --project <p> --environment <e> --name <n>` | Install MCP entry |
| `agent uninstall <client> --name <n>` | Remove MCP entry |
| `agent detect` | Detect installed MCP clients |
| `agent status` | Show installation status |
| `import-dbeaver <path-to-zip>` | Import from DBeaver export |
| `uninstall` | Remove SafeSelect entirely |

---

## MCP Tools

| Tool | Description | Arguments |
|---|---|---|
| `select` | Execute a SELECT query | `sql` (required) |
| `list_tables` | List database tables | `schema` (optional) |
| `explain` | Show execution plan | `sql` (required, not executed) |

---

## Configuration

Config is loaded from `$SAFESELECT_CONFIG_DIR` (default: `~/.config/safeselect/`).

```
config/
├── drivers/
│   └── postgresql.toml
└── projects/
    └── <name>/
        ├── project.toml          # security policy + limits
        └── environments/
            └── testing.toml      # connection + secrets
```

**project.toml** sets the maximum policy that no environment can relax:

```toml
version = 1
[security]
read_only = true
allowed_schemas = ["public"]
denied_relations = ["public.users_credentials"]
[limits]
statement_timeout_ms = 5000
max_rows = 500
max_result_bytes = 2_000_000
```

**environments/testing.toml** sets connection details:

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
```

---

## Detected AI Agents

- OpenCode (install supported)
- GitHub Copilot, Cursor, Windsurf, Claude Code, Codex, Gemini CLI (detected only)

---

## Requirements

- Rust 1.81+ (to build from source)
- Java 17+
- Maven 3.8+ (to rebuild the sidecar)

---

## Documentation

- [Installation guide](docs/install.md)
- [AI agent integration](docs/agents.md)
- [Security model](docs/security.md)
- [Distribution](docs/distribution.md)

---

## License

MIT – see [LICENSE](LICENSE).
