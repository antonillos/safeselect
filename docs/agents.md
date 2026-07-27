# AI Agent Integration

## Overview

SafeSelect implements the Model Context Protocol (MCP) over stdio, making it
compatible with any AI agent that supports MCP tools. It is designed for agents
that need database context while coding, debugging, refactoring, or reviewing SQL
without giving the agent direct database credentials or write access.

Product direction for agents:
- Read-only and fail-closed always come first.
- Prefer convention over configuration whenever the project or environment can be inferred safely.
- When automation cannot finish setup, SafeSelect should return the exact next safe step.
- Agent-ready workflows take priority over manual-only ergonomics.

Agents should treat SafeSelect as their database boundary:
- Use SafeSelect MCP tools only; SafeSelect does not expose MCP resources, so `list_mcp_resources` is not a database discovery step.
- For SQL, use `list_tables` then `describe_table`; never guess column names.
- For MongoDB, use `list_databases`, `list_collections`, then `discover_document_schema`; never guess field names.
- Follow `next_suggestion` from discovery results instead of repeating an invalid query unchanged.
- Use `select` only for small, targeted read-only queries.
- Place SQL CTEs at the beginning of the statement; do not nest `WITH` inside a
  subquery.
- Use `explain` to inspect query plans, index usage, and bottlenecks.
- Use `check` or `reconnect` before retrying after connection or SSH tunnel errors.
- Never ask the user for database passwords if `config_set_password` or existing config can resolve them.
- Prefer SafeSelect guidance output over inventing ad-hoc setup steps.

## Supported Agents

- **OpenCode** — fully supported with `safeselect agent install`
- **Cursor** — config-based MCP integration
- **Windsurf** — config-based MCP integration
- **Claude Code** — config-based MCP integration
- **OpenAI Codex** — config-based MCP integration
- **GitHub Copilot** — VS Code MCP support
- **Gemini CLI** — MCP support

## Installing in an Agent

```bash
# Detect available clients
safeselect agent detect

# Install in OpenCode
safeselect agent install opencode --project myapp --environment testing --name safeselect-myapp-testing

# Upgrade from the current project; name auto-detected when unambiguous
safeselect agent upgrade opencode --environment testing

# Or target a specific existing entry name explicitly
safeselect agent upgrade opencode --name safeselect-myapp-testing

# Check status
safeselect agent status
```

The installation command:

1. Detects the agent's config file.
2. For an interactive OpenCode install, offers the existing project-local config,
   a new `.opencode/opencode.jsonc` when `.opencode/opencode.json` already exists,
   or the global config.
3. Validates the selected config format and permissions.
4. Creates a backup.
5. Shows a diff of the change.
6. Writes the new config atomically and verifies it.

Use `safeselect agent upgrade` when you already have an installed SafeSelect MCP
entry and want to refresh it after upgrading the SafeSelect binary. By default it
migrates the entry to the canonical `safeselect-<project>-<environment>` name when
it can derive the project, and updates the generated MCP config in the same step.
If `--name` is omitted, SafeSelect resolves the entry from the current project and,
when needed, the provided `--environment`.

## Manual MCP Configuration

The installed entry looks like this in your agent's config:

```json
{
  "mcpServers": {
    "safeselect-myapp-testing": {
      "command": "safeselect",
      "args": ["serve", "--project", "myapp", "--environment", "testing"]
    }
  }
}
```

## Primary Query Tools

Use `database_info` first when the environment may not be SQL. It returns the
active backend, vendor, and capabilities. If the user only requested capability
information, report it and stop; do not continue into discovery or data access.

### `select`

Execute a read-only query and return JSON-serialized rows. The query is validated before execution:
- Must be read-only (`SELECT`, `EXPLAIN`, or `WITH`)
- Must be a single statement
- CTEs must be declared by a leading `WITH`; nested `WITH` clauses in subqueries
  are conservatively rejected
- Must respect schema allowlists and relation denylists
- Result row count and byte limits are enforced

Arguments:
- `sql` (required): SQL to execute
- `verbose` (optional): enable verbose sidecar logging for this execution

Successful responses are returned as MCP text content containing JSON with:
- `columns`: column names
- `rows`: row values
- `row_count`: number of returned rows
- `byte_count`: approximate payload bytes
- `elapsed_ms`: precise execution time in milliseconds
- `elapsed`: human-readable execution time

Recoverable PostgreSQL errors include a concrete safe next step when SafeSelect
can identify one. For example, if an aggregate or its ordinal position appears
in `GROUP BY`, remove it and group only by non-aggregate columns, or omit
`GROUP BY` when producing one aggregate result. For an incompatible operator,
rediscover the relation and compare `data_type` and `udt_name`; use operators
that match the observed types rather than adding a cast blindly. JSON and JSONB
values should use JSON operators such as `->` and `->>` against observed fields.

### `list_tables`

List database tables, optionally filtered by schema name. Use this before
describing an unfamiliar relation.

Arguments:
- `schema` (optional): schema name filter

The response preserves the standard SQL result fields and adds
`next_suggestion`. If the user requested data inspection, choose exactly one
`table_schema`/`table_name` pair and call `describe_table`. If the user only
requested the available relations or tools, report the result and stop. Do not
pass placeholders such as `<schema from list_tables>`, `*`, `%`, or another
wildcard.

### `describe_table`

Return ordered column metadata for one PostgreSQL table or view. SafeSelect
generates a fixed read-only `information_schema.columns` query internally; the
agent cannot provide SQL to this tool.

Arguments:
- `schema` (required): exact `table_schema` from one `list_tables` row
- `table` (required): exact `table_name` from the same row

Placeholders and wildcards are not supported. If a value such as
`<schema from list_tables>`, `*`, or `%` is provided, SafeSelect rejects the
call and directs the agent to copy one exact relation from `list_tables`.

Successful responses contain:
- `schema` and `table`: the described relation
- `columns`: ordered objects containing `column_name`, `data_type`, `udt_name`,
  `is_nullable`, `column_default`, and `ordinal_position`
- `column_count`, result byte/timing metadata, and `next_suggestion`

`udt_name` is PostgreSQL's underlying type name. It preserves information hidden
by generic `data_type` values; for example, a `jsonb[]` column has
`data_type: "ARRAY"` and `udt_name: "_jsonb"`.

Use only returned column names in the following `select` or `explain`. If the
relation is missing or inaccessible, follow the response's suggestion to call
`list_tables`; do not retry with guessed names. Schema allowlists and relation
denylists are checked before catalog access, and security violations remain
fail-closed.

### `explain`

Show the execution plan for a query. Defaults to:

```sql
EXPLAIN (FORMAT JSON) <sql>
```

This default is intentional: JSON plans are easier for agents to parse reliably.
Use `format: "text"` when the output is mainly for a human.

Arguments:
- `sql` (required): query to explain
- `analyze` (optional): execute the SELECT to collect actual runtime statistics
- `buffers` (optional): include cache/disk page activity
- `explain_verbose` (optional): include PostgreSQL `VERBOSE` planner output
- `format` (optional): `"json"` (default) or `"text"`
- `verbose` (optional): enable sidecar logging for this execution

For performance investigations, agents can request `analyze`, `buffers`, and
`explain_verbose` together. Because `ANALYZE` executes the SELECT, avoid it for
large or expensive queries unless the user is explicitly investigating performance.

### `list_databases`

List document databases for document-store backends.

Arguments: none

The response contains `databases` and a `next_suggestion` to call
`list_collections`.

### `list_collections`

List document collections in a database.

Arguments:
- `database` (required): database name

The response contains the filtered `collections` and a `next_suggestion` to
call `discover_document_schema`.

### `find_documents`

Find documents in a collection. The request is validated before execution:
- Must target an allowed database/collection when allowlists are configured
- Must not target denied collections
- `filter`, `projection`, and `sort` must be JSON objects
- Result document count and byte limits are enforced

Arguments:
- `database` (required): database name
- `collection` (required): collection name
- `filter` (required): JSON object filter
- `projection` (optional): JSON object projection
- `sort` (optional): JSON object sort
- `limit` (optional): maximum number of documents to return

### Additional MongoDB tools

- `aggregate_documents`: run a non-empty array of JSON-object stages; `$out` and `$merge` are rejected.
- `distinct_documents`: return distinct values for a field, optionally filtered and limited.
- `count_documents`: count documents matching a required, non-empty filter; `{}` is rejected to avoid accidental full scans.
- `explain_documents`: explain a bounded find query without executing a write.
- `profile_document_field`: profile a nested field over a bounded sample.
- `discover_document_schema`: infer frequent fields and types over a bounded,
  non-exhaustive sample. Its response includes `sampled_documents`,
  `schema_inference: "sampled_not_exhaustive"`, an explicit notice, and
  `next_suggestion`.
- `generate_document_fixture`: return anonymized samples in the response; it never writes fixture files.

All document tools enforce configured database/collection allowlists and denylists,
statement timeouts, and result-size limits.

MongoDB collections do not have an authoritative fixed schema. A field absent
from `discover_document_schema` may still exist outside the selected filter or
sample. Use observed fields for the next bounded `find_documents` or
`aggregate_documents` call, or inspect one field with `profile_document_field`.
If a field/path error is recoverable, rediscover the collection schema before
retrying; never broaden policy or limits automatically.

## Connection Tools

### `connect`

Reconnect to the configured database by re-establishing the backend connection.

### `disconnect`

Close the current backend connection.

### `reconnect`

Restart the Java sidecar process and verify the database connection. JDBC environments
use `SELECT 1`; document environments use a read-only backend ping.
Use this after tunnel changes, stale connections, sidecar timeouts, or recoverable
connection errors.

SafeSelect also auto-recovers from recoverable connection failures during query
execution by restarting the sidecar and retrying once. Agents should still call
`reconnect` when they need an explicit recovery step.

### `check`

Diagnose the configured environment from inside MCP. The response includes
stable diagnostic codes such as `SAFESELECT_CONFIG_RESOLVED`,
`SAFESELECT_SSH_BASTION_REACHABLE`, `SAFESELECT_SIDECAR_BACKEND_OK`, and
`SAFESELECT_BACKEND_VERIFICATION_OK` so agents can identify the failing layer before
trying a recovery action.

## Configuration Tools

These tools let an agent guide setup without leaving MCP. Destructive tools require
explicit confirmation arguments.

| Tool | Purpose | Arguments |
|---|---|---|
| `config_validate` | Validate project/environment config | `environment` optional |
| `config_show` | Show resolved config with secrets redacted | `environment` required |
| `config_set_password` | Store an environment password in macOS Keychain | `environment`, `password` |
| `config_rename_environment` | Rename an environment and migrate secret references | `old_name`, `new_name` |
| `config_delete_environment` | Delete one environment | `name` |
| `config_reset` | Delete all environments and keychain entries for the project | `confirm: true` |
| `driver_list` | List registered JDBC drivers | none |
| `driver_add` | Register a JDBC driver JAR | `vendor`, `path`, `class`, `sha256` optional |
| `driver_download` | Download/register the official PostgreSQL JDBC driver | `vendor: "postgresql"` |
| `agent_detect` | Detect installed MCP clients | none |
| `agent_install` | Install a SafeSelect MCP entry | `client`, `environment`, `name` optional |
| `agent_uninstall` | Remove a SafeSelect MCP entry | `client`, `name` |
| `agent_status` | Show SafeSelect install status for all clients | none |

### `import_compose`

Import PostgreSQL services discovered in docker-compose files. The MCP importer
creates `.safeselect/` config, records the SafeSelect version metadata, and
returns explicit next steps for driver setup, password setup, connectivity
verification, and agent installation.

### `uninstall`

Remove SafeSelect binary, config, data, audit logs, and keychain entries. Requires
`confirm: true`. Binary cleanup covers both `~/.local/bin/safeselect` from the
release installer and `~/.cargo/bin/safeselect` from `cargo install`.

## Agent Recovery Flow

When database access fails, agents should proceed in this order:

1. If a data tool returns `Connection closed`, stop probing data tools; call `check`.
2. Otherwise, call `check` and read the stable diagnostic codes.
3. If `check` reports `SAFESELECT_SIDECAR_CONNECTION_FAILED` while starting the sidecar, do not call `reconnect`; report the diagnostic and inspect config, tunnel, or backend availability.
4. If an existing sidecar, SSH tunnel, or backend connection is stale, call `reconnect` once.
5. If config is missing or invalid, call `config_validate` and `config_show`.
6. If the driver is missing, call `driver_list` then `driver_download` for PostgreSQL.
7. If the secret is missing, ask the user for permission/password and use `config_set_password`.
8. Do not retry rejected SQL after a security violation; SafeSelect intentionally exits fail-closed.

Timeouts are bounded by the project `statement_timeout_ms`. If a query times out,
agents should narrow filters, inspect the plan with `explain`, or ask the user
before increasing project limits.

## Security

- Each MCP entry is locked to a single project and environment
- Agents cannot change the target database
- Any security violation terminates the process
- All queries are audited (hashed, never stored in plain text)
