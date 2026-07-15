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
- Driver files must not be group- or world-writable; read access may be broader
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

### 4. MongoDB Validation Pipeline

Document operations use fixed, read-only tools rather than arbitrary commands. Database and
collection policy is checked before execution, aggregation rejects write stages such as `$out`
and `$merge`, counts require a non-empty filter, and every operation is bounded by result and
timeout limits. Profiling, schema discovery, and fixture generation operate on bounded samples;
fixtures are anonymized and returned in memory without writing files.

### 5. Backend Security

- Connection uses `READ ONLY` transaction mode
- `statement_timeout` prevents runaway queries
- Sidecar read timeouts respect `statement_timeout_ms` so MCP calls cannot hang indefinitely on zombie queries
- No `SET` statements or session modifications allowed
- `EXPLAIN ANALYZE` is allowed only through the read-only validation path; it executes the SELECT to collect runtime statistics but still cannot run DDL or DML
- MongoDB reconnect and health checks use a read-only ping; recovery retries a failed operation at most once

### 6. Fail-Closed

Any violation triggers:
1. Query cancellation
2. Backend connection close
3. Java sidecar termination
4. Audit log entry
5. MCP process exit

### 7. Audit Log

- Every query is logged as a SHA-256 hash
- Never: full SQL, credentials, secrets, DSN
- Format: JSON lines (`.jsonl`) with rotation
- If audit cannot initialize, the server refuses to start

### 8. Secret Management

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
| Agent attempts a MongoDB write stage | Fixed read-only tools and aggregation-stage validation |
| Agent requests an unbounded MongoDB count | Empty count filters are rejected |
