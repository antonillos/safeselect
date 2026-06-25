# SafeSelect

**MCP database fail-closed access for AI Agents**

[![CI](https://github.com/antonillos/safeselect/actions/workflows/verify.yml/badge.svg)](https://github.com/antonillos/safeselect/actions/workflows/verify.yml)
[![Security](https://img.shields.io/badge/Security-fail--closed-success?logo=trustpilot&logoColor=white)]()
[![Rust](https://img.shields.io/badge/Rust-1.81%2B-dea584?logo=rust&logoColor=white)]()
[![Java](https://img.shields.io/badge/Java-17%2B-5382a1?logo=openjdk&logoColor=white)]()
[![MCP](https://img.shields.io/badge/MCP-0.1.0-7b68ee)]()
[![Homebrew](https://img.shields.io/badge/Homebrew-tap-FBB040?logo=homebrew&logoColor=white)](https://github.com/antonillos/homebrew-tap)
[![asdf](https://img.shields.io/badge/asdf-plugin-8A2BE2)](https://github.com/antonillos/asdf-safeselect)
[![License](https://img.shields.io/badge/License-MIT-yellow)](LICENSE)

SafeSelect is a secure read-only boundary between AI coding agents and your databases. It supports SQL/JDBC backends and MongoDB document backends through the **Model Context Protocol (MCP)** with a fail-closed security model: security first, convention over configuration, and guided next steps when setup or recovery is needed.

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

# 2. Import
safeselect import-dbeaver ~/Downloads/dbeaver-export.zip
#    Or from docker-compose:
#    safeselect import-compose
#    Or from MongoDB Compass:
#    safeselect import-compass --path "$HOME/.config/MongoDB Compass"

#    Imports can prompt for:
#    - Environment names
#    - SSH/bastion config
#    - Database passwords
#    - SSH passwords when Compass/DBeaver did not export them

# 3. Verify connectivity (auto-establishes SSH tunnel if needed)
safeselect check --environment testing

# 4. Connect your AI agent (name auto-generates as safeselect-<project>-testing)
safeselect agent install opencode --environment testing
```

Release notes are published from `CHANGELOG.md`, so each GitHub release includes
a short summary of what changed plus the install instructions.

---

## Security Model

- **Fail-closed**: any security violation kills the MCP process immediately
- **Read-only**: SQL allows only `SELECT`, `EXPLAIN`, and `WITH`; MongoDB backends allow discovery and `find_documents`
- **Single statement**: multi-statement SQL rejected
- **Scope control**: allow/deny SQL schemas/relations and document databases/collections
- **SHA-256 drivers**: JDBC JAR checksummed on every use
- **macOS Keychain**: secrets never stored in config files
- **Password isolation**: passed via stdin, never as CLI args
- **Result limits**: row count and byte size enforced

## Product Principles

- **Read-only, security first**: SafeSelect exists to keep AI agent database access strictly non-mutating.
- **Convention over configuration**: prefer inferred project, environment, driver, MCP entry name, and import defaults.
- **Wizard and next steps**: when setup cannot be fully automated, SafeSelect should guide the user or agent with the exact next safe action.
- **Agent-ready first**: the MCP experience is a primary surface, not an afterthought.
- **Developer adoption**: setup, diagnosis, and recovery should reduce friction for everyday development teams.

---

## Read-Only Use Cases

- **Schema discovery**: use `list_tables` for SQL, or `list_databases`/`list_collections` for document stores, before guessing names.
- **Safe data inspection**: inspect rows, counts, shapes, and join paths without exposing write access.
- **Performance analysis**: use `explain` and `explain analyze` on read-only queries to inspect plans, buffers, and bottlenecks.
- **Connectivity diagnosis**: use `check`, `connect`, and `reconnect` to recover from stale JDBC, sidecar, or SSH tunnel state.
- **Agent onboarding**: install a SafeSelect MCP entry so agents get a constrained, project-scoped, read-only database tool by default.
- **Project bootstrap**: import from DBeaver, docker-compose, or MongoDB Compass, then follow the generated next steps to finish setup.

---

## CLI Reference

| Command | Description |
|---|---|
| `serve --project <p> --environment <e>` | Start the MCP server |
| `query --project <p> --environment <e> --sql <q>` | Execute SQL directly |
| `check --project <p> --environment <e>` | Test connectivity |
| `doctor --project <p> --environment <e>` | Diagnose config, SSH tunnel, sidecar, and backend connectivity |
| `config validate [--project <p>] [--environment <e>]` | Validate config |
| `config show --project <p> --environment <e>` | Show resolved config |
| `config rename-environment --old <o> --new <n>` | Rename environment |
| `config delete-environment --name <n>` | Delete environment |
| `config set-password --environment <e>` | Store password in Keychain and update config |
| `config set-ssh-password --environment <e>` | Store SSH password in Keychain and update SSH config |
| `config reset [--project <p>]` | Remove all environments + keychain entries |
| `driver download --vendor postgresql` | Download JDBC driver |
| `driver add --vendor <v> --path <jar> --class <c>` | Register custom driver |
| `driver list` | List registered drivers |
| `import-compass [--path <p>]` | Import MongoDB Compass connections |
| `agent install <client> --environment <e> [--project <p>] [--name <n>]` | Install MCP entry (name defaults to `safeselect-<project-dir>-<environment>`) |
| `agent upgrade <client> [--name <n>] [--project <p>] [--environment <e>]` | Upgrade an existing MCP entry, auto-detecting it from the current project when possible |
| `agent uninstall <client> --name <n>` | Remove MCP entry |
| `agent detect` | Detect installed MCP clients |
| `agent status` | Show installation status |
| `import-dbeaver <path-to-zip> [--non-interactive]` | Import from DBeaver export (interactive wizard with SSH setup) |
| `import-compose [--path <yml>] [--non-interactive]` | Import from docker-compose with convention-based detection, next steps, and password guidance |
| `connect --project <p> --environment <e>` | Reconnect to database |
| `disconnect --project <p> --environment <e>` | Disconnect from database |
| `uninstall` | Remove SafeSelect entirely |

---

## Diagnostics

`check` and `doctor` report each phase with a stable diagnostic code so humans
and AI agents can identify exactly where a failure happens without changing the
normal connection behavior.

Example phases:

- `SAFESELECT_CONFIG_RESOLVED`
- `SAFESELECT_DRIVER_VERIFIED`
- `SAFESELECT_SECRET_RESOLVED`
- `SAFESELECT_SSH_BASTION_REACHABLE`
- `SAFESELECT_POSTGRES_REACHABLE`
- `SAFESELECT_SIDECAR_BACKEND_OK`
- `SAFESELECT_BACKEND_VERIFICATION_OK`

Use these codes when reporting issues or asking an agent to recover a broken
environment.

MCP query responses include execution metadata for agent decisions:

- `row_count` and `byte_count` for result sizing
- `elapsed_ms` for precise timing
- `elapsed` for human-readable timing such as `842ms`, `1.3s`, or `2m 4s`

Sidecar pipe timeouts and MCP reconnect behavior respect the configured
limits. Agents should prefer `check`, `connect`, and `reconnect`
over manual retry loops after stale SSH or sidecar connections.

---

## MCP Tools

| Tool | Description | Arguments |
|---|---|---|
| `database_info` | Show active backend, vendor, and capabilities | _(none)_ |
| `select` | Execute a SELECT query | `sql` (required), `verbose` |
| `list_tables` | List database tables | `schema` (optional) |
| `explain` | Show execution plan | `sql` (required), `analyze`, `buffers`, `explain_verbose`, `format` (`json` default, `text`) |
| `list_databases` | List document databases | _(none)_ |
| `list_collections` | List document collections | `database` |
| `find_documents` | Find documents in a collection | `database`, `collection`, `filter`, `projection`, `sort`, `limit` |
| `connect` | Reconnect to the database after connection loss | _(none)_ |
| `disconnect` | Close the database connection | _(none)_ |
| `reconnect` | Restart the sidecar and verify the database connection | _(none)_ |
| `check` | Diagnose MCP database connectivity | _(none)_ |

Configuration and setup tools available through MCP:

| Tool | Description | Arguments |
|---|---|---|
| `config_validate` | Validate `.safeselect/` configuration | `environment` (optional) |
| `config_show` | Show resolved config with secrets redacted | `environment` (required) |
| `config_set_password` | Store an environment password in Keychain | `environment`, `password` |
| `config_set_ssh_password` | Store an SSH password in Keychain | `environment`, `password` |
| `config_rename_environment` | Rename an environment | `old_name`, `new_name` |
| `config_delete_environment` | Delete an environment | `name` |
| `config_reset` | Delete all project environments and keychain entries | `confirm: true` |
| `driver_list` | List registered JDBC drivers | _(none)_ |
| `driver_add` | Register a JDBC driver | `vendor`, `path`, `class`, `sha256` (optional) |
| `driver_download` | Download/register PostgreSQL JDBC driver | `vendor: "postgresql"` |
| `agent_detect` | Detect installed MCP clients | _(none)_ |
| `agent_install` | Install a SafeSelect MCP entry | `client`, `environment`, `name` (optional) |
| `agent_uninstall` | Remove a SafeSelect MCP entry | `client`, `name` |
| `agent_status` | Show SafeSelect install status for clients | _(none)_ |
| `import_compose` | Scan docker-compose files, import PostgreSQL services, and return guided next steps | `scan_path` (optional) |
| `uninstall` | Remove SafeSelect binary/config/data/audit/keychain | `confirm: true` |

Setup mode tools (available when no `.safeselect/` is found and `safeselect serve --environment <env>` enters setup mode automatically):

| Tool | Description | Arguments |
|---|---|---|
| `import_compose` | Scan docker-compose files, import PostgreSQL services, and return guided next steps | `scan_path` (optional) |
| `delete_environment` | Delete an environment configuration | `name` (required) |
| `rename_environment` | Rename an environment (migrates secret reference) | `old_name` (required), `new_name` (required) |

---

## Configuration

Global config lives in `$SAFESELECT_CONFIG_DIR` (default: `~/.config/safeselect/`),
shared across all projects (drivers, sidecar).

```
~/.config/safeselect/
├── drivers/
│   └── postgresql.toml           # registered JDBC drivers
└── sidecar/
    └── safeselect-sidecar.jar    # embedded Java sidecar
```

Each project (git repo) carries its own `.safeselect/` directory:

```
<repo-root>/
└── .safeselect/
    ├── project.toml              # security policy + limits
    └── environments/
        └── <env>.toml            # connection + secrets
```

Commands auto-detect `.safeselect/` by walking up from the current directory.
Use `--project <path>` to point to a specific repo root.

**project.toml** sets the maximum policy that no environment can relax:

```toml
version = 1
[security]
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
url = "jdbc:postgresql://localhost:15432/myapp?sslmode=require"
username = "reader"
```

For MongoDB Compass imports, SafeSelect rewrites the URL to a local forwarded endpoint,
resolves `mongodb+srv` tunnel targets to real TCP hosts/ports, and keeps SSH and MongoDB
local ports separate.

When SSH tunneling is configured, the database URL uses a local forwarding port such as
`localhost:15432`. The SSH server port and forwarding port are intentionally different.

The password is configured separately — run this once:

```bash
safeselect config set-password --environment testing
```

This stores the password in your macOS Keychain and adds the `[database.secret]` section to the toml automatically.

If the bastion uses password auth and the import did not include it:

```bash
safeselect config set-ssh-password --environment testing
```

### SSH tunnels

If your DBeaver connection uses an SSH tunnel, SafeSelect will prompt for
SSH configuration during import:

- **SSH bastion host**: the jump host (e.g., `localhost` if using a local forward)
- **SSH port**: the SSH server port (e.g., `2222`)
- **SSH user**: the SSH username (e.g., `jumpboxdev`)
- **Auth method**: key file or password

For password-based SSH auth, install `sshpass` for automatic tunnel setup:
```bash
brew install <your-tap>/sshpass
```

SafeSelect forwards local port `15432` to the database through the SSH tunnel
and uses `sslmode=require` for Azure PostgreSQL compatibility.

When a release is prepared, SafeSelect also updates `CHANGELOG.md` and uses the
matching version entry as the GitHub Release notes body.

Tunnel health is verified in two steps:
1. **Bastion reachability**: TCP check to the SSH server endpoint
2. **PostgreSQL protocol check**: SSLRequest to verify the target responds as PostgreSQL

### Version tracking

`project.toml` stores a `generated_by` field with the SafeSelect version that
created the config. When reimporting with a different version, you will be
prompted to reset environments first.

### Advanced: Manual secret setup

If you prefer to configure secrets by hand, add this to the environment toml:

```toml
[database.secret]
source = "macos-keychain"
service = "safeselect"
account = "myapp/testing"
```

Then store the password in the Keychain:

```bash
security add-generic-password -a "myapp/testing" -s "safeselect" -w "<password>"
```

For non-macOS systems, use an environment variable instead:

```toml
[database.secret]
source = "env"
variable = "SAFESELECT_PASSWORD_TESTING"
```

Then export the variable before running `safeselect`.

---

## Detected AI Agents

- OpenCode (install supported)
- GitHub Copilot, Cursor, Windsurf, Claude Code, Codex, Gemini CLI (detected only)

---

## Requirements

- Rust 1.81+ (to build from source)
- Java 17+
- Maven 3.8+ (to rebuild the sidecar)
- `sshpass` (optional, for automatic password-based SSH tunnel setup)

To build from source:

```bash
./install.sh                    # builds sidecar + Rust binary, installs to ~/.local/bin
safeselect --version            # shows e.g. "safeselect 0.4.0 (2026.06.23.21.30)"
```

---

## Documentation

- [Installation guide](docs/install.md)
- [AI agent integration](docs/agents.md)
- [Security model](docs/security.md)
- [Distribution](docs/distribution.md)
- [Changelog](CHANGELOG.md)

---

## License

MIT – see [LICENSE](LICENSE).
