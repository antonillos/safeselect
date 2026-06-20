# Security Model

## Principles

SafeSelect is built on a **fail-closed** security model. When in doubt, shut
down. No graceful degradation, no fallback, no second chances.

## Security Layers

### 1. Configuration Validation

Before starting, SafeSelect validates:
- TOML format and required fields
- File permissions (no group/world-writable config files)
- No symlinks in config paths
- No credentials in config files or JDBC URLs
- Secret source is properly configured (Keychain or env var)

### 2. Driver Validation

Every time the server starts:
- Driver `.jar` file must exist
- File permissions must be secure (owner-only)
- SHA-256 checksum is verified against the registered hash
- Any mismatch prevents startup

### 3. SQL Validation Pipeline

Each query goes through:

```
Raw SQL → Size check → Single statement check → Read-only check
→ Schema allowlist → Relation denylist → JDBC execution → Result limits
```

| Check | What it prevents |
|---|---|
| Size limit | DoS via oversized queries |
| Single statement | SQL injection via stacked queries |
| Read-only | DDL, DML, and destructive operations |
| Schema allowlist | Access to schemas outside policy |
| Relation denylist | Access to sensitive tables |

### 4. JDBC Security

- Connection uses `READ ONLY` transaction mode
- `statement_timeout` prevents runaway queries
- Sidecar read timeouts respect `statement_timeout_ms` so MCP calls cannot hang indefinitely on zombie queries
- No `SET` statements or session modifications allowed
- `EXPLAIN ANALYZE` is allowed only through the read-only validation path; it executes the SELECT to collect runtime statistics but still cannot run DDL or DML

### 5. Fail-Closed

Any violation triggers:
1. Query cancellation
2. JDBC connection close
3. Java sidecar termination
4. Audit log entry
5. MCP process exit

### 6. Audit Log

- Every query is logged as a SHA-256 hash
- Never: full SQL, credentials, secrets, DSN
- Format: JSON lines (`.jsonl`) with rotation
- If audit cannot initialize, the server refuses to start

### 7. Secret Management

- Sources: macOS Keychain or environment variables (never inline)
- Resolved once at startup, held in memory
- Never written to disk or log files
- Passwords passed to Java sidecar via stdin (not CLI args)

## Threat Model

| Threat | Mitigation |
|---|---|
| Agent tries to DROP a table | Read-only enforcement |
| Agent accesses `users_credentials` | Denied relations |
| Agent sends `SELECT 1; DROP TABLE users` | Single statement check |
| Malicious driver JAR | SHA-256 checksum |
| Process memory dump | Secret not on CLI args |
| Unauthorized config modification | Permission check + backup |
| Agent needs query tuning | `EXPLAIN` defaults to JSON plans; `ANALYZE`, `BUFFERS`, and `VERBOSE` are explicit options |
