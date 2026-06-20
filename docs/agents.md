# AI Agent Integration

## Overview

SafeSelect implements the Model Context Protocol (MCP) over stdio, making it
compatible with any AI agent that supports MCP tools.

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
safeselect agent install opencode --project myapp --environment testing --name myapp-testing

# Check status
safeselect agent status
```

The installation command:
1. Detects the agent's config file
2. Validates the config format and permissions
3. Creates a backup
4. Shows a diff of the change
5. Writes the new config atomically
6. Verifies the write

## Manual MCP Configuration

The installed entry looks like this in your agent's config:

```json
{
  "mcpServers": {
    "myapp-testing": {
      "command": "safeselect",
      "args": ["serve", "--project", "myapp", "--environment", "testing"]
    }
  }
}
```

## Tools Available to Agents

### `select`

Execute a SELECT query. The query is validated before execution:
- Must be read-only (SELECT or EXPLAIN)
- Must be a single statement
- Must respect schema allowlists and relation denylists

### `list_tables`

List database tables, optionally filtered by schema name.

### `explain`

Show the execution plan for a query without executing it.

### `check`

Diagnose the configured environment from inside MCP. The response includes
stable diagnostic codes such as `SAFESELECT_CONFIG_RESOLVED`,
`SAFESELECT_SSH_BASTION_REACHABLE`, `SAFESELECT_SIDECAR_JDBC_OK`, and
`SAFESELECT_QUERY_SELECT_ONE_OK` so agents can identify the failing layer before
trying a recovery action.

### `reconnect`

Restart the sidecar and verify the database connection after connection loss or
an SSH tunnel change.

### `import_compose`

Import PostgreSQL services discovered in docker-compose files. The MCP importer
creates `.safeselect/` config and records the SafeSelect version metadata; run
`check` afterwards to verify driver, secret, SSH, sidecar, and `SELECT 1`.

## Security

- Each MCP entry is locked to a single project and environment
- Agents cannot change the target database
- Any security violation terminates the process
- All queries are audited (hashed, never stored in plain text)
