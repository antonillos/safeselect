---
name: safeselect-setup
description: >-
  Use when installing or configuring SafeSelect database access, importing connections, registering MCP and skills in Codex or OpenCode, resolving missing drivers or secrets, or diagnosing SSH and connection failures.
---

# SafeSelect setup and recovery

Use SafeSelect MCP tools first, then supported CLI commands. Inspect current
configuration before changing it; registration and configuration are writes.
Read [setup recipes and effects](references/workflows.md) before making changes.

1. Resolve the application repository and environment. Never guess among multiple
   environments. `config_validate` and redacted `config_show` diagnose config.
2. Use `check` for connection failures and follow its stable diagnostic codes.
3. For an existing stale sidecar/JDBC/SSH connection, use `reconnect` once.
   If `SAFESELECT_SIDECAR_CONNECTION_FAILED` occurs during startup, report it and
   inspect configuration, tunnel and backend availability instead of reconnecting.
4. For missing JDBC drivers, use `driver_list`; `driver_download` supports
   PostgreSQL. Register other trusted JARs with `driver_add` and SHA-256 checking.
5. Reuse configured secret sources. Never print passwords, put them in JDBC URLs,
   process arguments, committed files or reports. Use `config_set_password` only
   when the user authorizes storing a secret through the available secure channel.
6. Change configuration or integration only within the user's requested scope.
   Do not invoke destructive config or uninstall tools merely to diagnose failure.

CLI connection commands do not change the agent's active MCP session. A terminated
server must be restarted by its host after the cause is corrected.
