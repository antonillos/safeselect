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
2. If an external bastion, VPN, or tunnel is known unavailable, stop and ask the
   user to restore it manually. Do not call `check`, `connect` or `reconnect`, or
   try shell SSH/VPN commands; wait for the user's confirmation before resuming.
3. With that route available and only the database session closed/disconnected,
   call `reconnect` once directly, without `check` first. Retry the original
   bounded operation once only after success. Stop after failed recovery; never
   alternate `check` and `reconnect`. If route availability is unknown, use
   `check` at most once to distinguish the cause. For a startup
   `SAFESELECT_SIDECAR_CONNECTION_FAILED`, report the diagnostic instead of
   treating it as an established database session that merely needs reconnecting.
4. For missing JDBC drivers, use `driver_list`; `driver_download` supports
   PostgreSQL. Register other trusted JARs with `driver_add` and SHA-256 checking.
5. Reuse configured secret sources. Never print passwords, put them in JDBC URLs,
   process arguments, committed files or reports. Use `config_set_password` only
   when the user authorizes storing a secret through the available secure channel.
6. Change configuration or integration only within the user's requested scope.
   Do not invoke destructive config or uninstall tools merely to diagnose failure.

CLI connection commands do not change the agent's active MCP session. A terminated
server must be restarted by its host after the cause is corrected.
