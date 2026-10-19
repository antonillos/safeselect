# Security Model

## Principles

SafeSelect is built on a **fail-closed** security model. Rejected client requests never execute. Ordinary policy rejections return an error while the MCP session stays available. Serious security attempts and internal validation or audit failures stop the server; there is no fallback around policy enforcement.

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
Raw SQL → Empty/size/comment checks → Serious operation scan
→ Single statement check → Read-only check → System catalog restriction
→ Schema allowlist → Relation denylist → JDBC execution → Result limits
```

| Check | What it prevents |
|---|---|
| Size limit | DoS via oversized queries |
| Serious operation scan | Writes, transaction/session changes, and forbidden state-changing functions |
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
- Database roles may retain write privileges; SafeSelect does not change grants or reject those
  credentials, but every JDBC query it issues runs in a verified `READ ONLY` transaction
- `statement_timeout` prevents runaway queries
- Sidecar read timeouts respect `statement_timeout_ms` so MCP calls cannot hang indefinitely on zombie queries
- No `SET` statements or session modifications allowed
- `EXPLAIN ANALYZE` is allowed only through the read-only validation path; it executes the SELECT to collect runtime statistics but still cannot run DDL or DML
- MongoDB reconnect and health checks use a read-only ping; recovery retries a failed operation at most once

### 6. Fail-Closed

Fail-closed means a rejected operation cannot execute. A rejected request does
not always require terminating the MCP process. SQL validation distinguishes
ordinary request rejections from serious security attempts using typed errors,
not an agent's interpretation of the error message or the caller's intent.

#### Ordinary rejections: keep the session available

These requests return an error without executing the rejected SQL. Subsequent
valid calls can use the same MCP process, and tools such as `list_tables`,
`check` and `reconnect` remain available.

| Request | Result |
|---|---|
| `describe_table` with schema `information_schema` and table `views` | Reserved catalog rejected; session stays available |
| `select` or `explain` targeting `information_schema.views` | Catalog access rejected; session stays available |
| A relation outside `allowed_schemas` or listed in `denied_relations` | Scope restriction enforced; session stays available |
| Empty SQL, SQL exceeding 102,400 bytes, or unsupported nested block comments | Input rejected; session stays available |
| Unrecognized SQL such as `not valid SQL` | Request rejected; session stays available |
| `SELECT 1; SELECT 2` with `require_single_statement = true` | Multiple statements rejected; session stays available |
| `SELECT pg_sleep(1)` | Forbidden sleep function rejected; session stays available |

Relation-target rejections remain recoverable for `describe_table`,
`list_table_indexes`, `list_table_partitions` and `get_table_stats`. Malformed
arguments, such as missing names or wildcard table names, also return errors
without closing the session. Allowing the session to continue does not grant
access to the rejected relation or relax any configured policy.

#### Serious SQL attempts: terminate the session

For `select` and `explain`, the following detected operations produce
`SafeselectError::Security`. SafeSelect records the rejection, attempts to send
the JSON-RPC error, kills the sidecar if one exists, and exits with status 1.

| Attempt | Examples |
|---|---|
| Writes or DDL | `INSERT`, `UPDATE`, `DELETE`, `DROP`, `ALTER`, `TRUNCATE` |
| Permission or session changes | `GRANT`, `REVOKE`, `SET ROLE`, `SET_CONFIG` |
| Transaction control or administrative operations | `COMMIT`, `ROLLBACK`, `LOCK`, `VACUUM` |
| Forbidden state-changing functions | `SETVAL`, `NEXTVAL`, advisory locks, `PG_TERMINATE_BACKEND`, file import/export functions |
| A destructive statement combined with a read | `SELECT 1; DROP TABLE public.example` |
| A write hidden in a CTE or passed to `explain` | `WITH removed AS (DELETE FROM public.example RETURNING *) SELECT * FROM removed` |

The serious-operation scan runs before statement-count and relation-scope
checks, after the initial empty, size and comment checks. Consequently,
`DELETE FROM information_schema.views` is a serious attempt even though a
read of the same catalog is an ordinary rejection. A read-only prefix cannot
turn a subsequent detected write into a recoverable error. SQL string literals
are excluded from the keyword scan: `SELECT 'DROP TABLE public.example'` does
not itself constitute a destructive operation.

The supported checks and forbidden operation lists are defined in
[`src/security.rs`](../src/security.rs). They are deterministic policy checks,
not a general assessment of malicious intent. A blocked function such as
`pg_sleep` remains forbidden even though its rejection is recoverable.

#### Internal failures and scope

Failure to validate internally generated catalog SQL still terminates the
sidecar and MCP process after recording the rejection. This is an internal
safety failure rather than an ordinary relation-target rejection. Audit
failures also stop request processing; the server must not continue without
its mandatory audit trail. Configuration and driver-integrity failures can
prevent startup. Transport and I/O failures can also end a session.

This distinction applies to the SQL MCP request paths. MongoDB validation,
read-only transactions, driver SHA-256 checks, credential handling and result
limits are unchanged. There is no new configuration switch or exception that
permits a rejected operation to execute.

#### Recovery and MCP responses

After an ordinary rejection, follow `next_suggestion` and correct the request
within policy. For a reserved catalog, use the dedicated discovery tools and
choose an allowed relation from `list_tables`. For query planning, call
`explain` with `analyze=false` and an allowed SELECT. Do not reconnect solely
because a request was rejected, repeat the unchanged request, widen policy,
or switch to another environment to bypass the rejection.

Both ordinary SQL policy rejections and serious SQL security errors use
JSON-RPC error code `-32000`, preserve the request ID and include
`error.data.no_data = true` with trusted `next_suggestion` guidance. The error
code alone does not identify whether the process will terminate. Successful
audit recording precedes the error response; audit failures can prevent that
response from being sent. Rejection audit entries use category `REJECT`,
decision `reject` and a query hash, without storing the SQL text.

`reconnect` restarts the sidecar inside a running MCP process. It cannot revive
a terminated MCP server, because that server's tools are no longer callable.
After a serious security attempt or fatal internal failure, correct the cause
and restart the affected MCP entry through the host application's server
lifecycle controls. For stale connections in a live session, follow the
[`check`/`reconnect` flow](agents.md#agent-recovery-flow).

#### Regression coverage

[`src/security.rs`](../src/security.rs) tests classification, including mixed
read/write statements, CTE writes, EXPLAIN targets, recoverable errors and SQL
string literals. [`tests/mcp_negative.rs`](../tests/mcp_negative.rs) verifies
that ordinary rejections preserve the process and tool discovery, subsequent
allowed requests reach the backend path, and serious attempts produce an
audited error followed by exit status 1. These process tests use an unavailable
backend fixture; they do not establish behavior against a live database.
[`tests/smoke.rs`](../tests/smoke.rs) also checks that a destructive stacked
query is audited before termination. See the
[security suite contract](security-test-suite.md) for real-database validation.

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
`explain`, stale connections point to `check`/`reconnect`, and ordinary policy rejections require correcting the request. Serious security attempts, internal validation, audit and
startup failures stop the server. Database-derived detail remains UUID-delimited;
agents must not retry an unchanged request. The same trusted suggestion is
also appended to the JSON-RPC error message for MCP clients that render only
the compact error summary; database-derived detail is never appended there.

### 9. Secret Management

- Sources: macOS Keychain or environment variables (never inline)
- Resolved once at startup, held in memory
- Never written to disk or log files
- Database passwords are passed to the Java sidecar via stdin; SSH passwords are
  supplied to `sshpass` through its environment variable, never through process arguments

When `allowed_schemas` is configured, every database relation in agent-provided SQL must use
an explicit `schema.table` name. CTE aliases and derived tables remain valid without a schema;
unqualified database relations are rejected rather than resolved through `search_path`.

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
