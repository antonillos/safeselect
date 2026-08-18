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
and `$merge`, and every filter, projection, sort, and pipeline is recursively checked for
server-side JavaScript. `$where`, `$function`, and `$accumulator` are rejected before BSON
conversion in Rust and independently in the Java sidecar; their bodies are neither audited nor
returned. Counts require a non-empty filter, and every operation is bounded by result and timeout
limits; MongoDB driver commands receive that timeout as `maxTimeMS`. Profiling, schema discovery, and fixture generation operate on bounded samples; fixtures
are anonymized and returned in memory without writing files. There is no configuration switch to
enable JavaScript: rejected requests must be rebuilt with declarative MQL operators.

Index and statistics tools use the same database/collection policy, command timeout, audit trail,
and result-byte bound. They expose an explicit allowlist of index and storage fields rather than
forwarding raw `listIndexes`, `listSearchIndexes`, `dbStats`, or `collStats` command documents.
Atlas Search capability failures are reduced to `unsupported` or `unauthorized`; unexpected
Search failures fail the tool closed.

PostgreSQL index and statistics tools use fixed read-only catalog queries and
expose only documented index, size, count, and scan fields. They require an
exact relation that passes the existing schema allowlist and relation denylist;
they never accept arbitrary catalog SQL or return raw catalog rows.

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

- Every operation is logged with a SHA-256 query hash and bounded metadata
- The current MCP session exposes `audit_status` and `audit_recent`; the latter
  is capped at 20 entries and identifies the operation tool when available
- Never: full SQL, credentials, secrets, DSN
- Never: returned documents, filters, local paths, or audit events from earlier sessions
- Format: JSON lines (`.jsonl`) with rotation
- If audit cannot initialize, the server refuses to start

### 8. MCP Error Guidance

Every MCP error carries exactly one contextual `next_suggestion`. Invalid
arguments identify the correction, timeouts point to a narrower query and
`explain`, stale connections point to `check`/`reconnect`, and security or
startup failures are terminal. Database-derived detail remains UUID-delimited;
agents must not retry an unchanged request. The same trusted suggestion is
also appended to the JSON-RPC error message for MCP clients that render only
the compact error summary; database-derived detail is never appended there.

### 9. Secret Management

- Sources: macOS Keychain or environment variables (never inline)
- Resolved once at startup, held in memory
- Never written to disk or log files
- Database passwords are passed to the Java sidecar via stdin; SSH passwords are
  supplied to `sshpass` through its environment variable, never through process arguments

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
| Agent attempts MongoDB server-side JavaScript | Recursive `$where`, `$function`, and `$accumulator` rejection in Rust and Java before driver execution |
| Agent requests an unbounded MongoDB count | Empty count filters are rejected |
