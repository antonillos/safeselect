# SQL rejection classification

The classifier determines whether SQL may continue to the existing relation and
backend checks, must be rejected while keeping MCP available, or constitutes a
serious operation that terminates the MCP session. These are deterministic
operation classes, not conclusions about a caller's intent.

## Classification matrix

| Class | Examples or boundary | Result |
|---|---|---|
| Read query | `SELECT`, `TABLE`, read-only CTEs, subqueries and set operations | Continue to policy checks |
| Read query plan | `EXPLAIN`, including `ANALYZE`, over a permitted read | Continue to policy checks |
| Data rather than commands | `replace(...)`, columns named `start` or `commit`, a table named `lock` | Continue to policy checks |
| Function-name fragments | `nextval_count`, `setrole_count`, `pg_sleep_count`, `public.nextval_count()` | Continue to policy checks |
| Quoted data | Comments, string literals, dollar-quoted literals, quoted column names | Contents do not become commands |
| Unsupported input | Empty, oversized, malformed or unsupported SQL; actual nested block comments | Recoverable rejection |
| Unsupported read commands | Standalone `SHOW` and `VALUES` | Recoverable rejection |
| Multiple reads | `SELECT 1; SELECT 2` with single-statement policy enabled | Recoverable rejection |
| Scope restrictions | Reserved catalogs, disallowed schemas, denied relations | Recoverable rejection |
| Sleep routines | `pg_sleep`, `pg_sleep_for`, `pg_sleep_until` | Recoverable rejection |
| Writes and object changes | DML, DDL, `SELECT INTO`, `COMMENT`, `SECURITY LABEL`, `IMPORT FOREIGN SCHEMA` | Serious rejection |
| Authorization changes | `GRANT`, `REVOKE`, `REASSIGN OWNED`, session authorization changes | Serious rejection |
| Transaction control | `BEGIN`, `START`, `COMMIT`/`END`, `ROLLBACK`/`ABORT`, savepoints | Serious rejection |
| Session and cursor state | `SET`, `RESET`, `DISCARD`, `PREPARE`, `EXECUTE`, `DEALLOCATE`, `DECLARE`, `FETCH`, `MOVE`, `CLOSE` | Serious rejection |
| Notifications | `LISTEN`, `UNLISTEN`, `NOTIFY`, `pg_notify(...)` | Serious rejection |
| Locks | `LOCK`, locking SELECT clauses, advisory lock/unlock routines | Serious rejection |
| Maintenance and administration | `ANALYZE`/`ANALYSE`, `VACUUM`, `REINDEX`, `CLUSTER`, `REFRESH MATERIALIZED VIEW`, `CHECKPOINT`, `LOAD` | Serious rejection |
| External or procedural execution | `COPY`, `DO`, `CALL` | Serious rejection |
| Known state-changing routines | Sequence changes, session configuration, backend control, replication-slot changes, backup/WAL control, large-object writes | Serious rejection |

Cursor reads such as `FETCH` advance server-side state; they belong to the same
serious class as cursor creation, movement and closure. `ANALYZE` as a command
changes statistics; `EXPLAIN ANALYZE` is a plan wrapper whose target is checked.
Sleep routines remain an explicit recoverable exception and are never executed.

## Context and precedence

The PostgreSQL tokenizer identifies real statement boundaries. Semicolons inside
comments or literals, including tagged dollar quotes, do not split statements.
Command names are checked only at statement starts. The parsed syntax tree then
identifies nested statements, SELECT creation clauses, row locks and function
calls. Function checks compare complete PostgreSQL identifiers: unquoted names
fold to lower case, while quoted names retain case. Qualification does not exempt
a forbidden function. Columns, CTE names and aliases are not function calls.

Every tokenizable statement is checked before the single-statement and relation
policies. A detected serious operation wins over an ordinary rejection in the
same request, including a sleep call or malformed earlier statement. CTE writes,
subqueries, table functions, set-operation branches and EXPLAIN targets receive
the same checks. A listed command becomes serious only after its complete
statement parses or matches an explicit PostgreSQL compatibility grammar.
Malformed requests such as `DELETE nonsense` and `SECURITY nonsense` are
recoverable, including inside EXPLAIN and alongside other harmless statements.
EXPLAIN compatibility checks validate the wrapper and target separately.

The [compatibility grammar](../src/security/sql_classification/postgres_commands.rs)
recognizes complete common forms of `CHECKPOINT`, `REFRESH MATERIALIZED VIEW`,
`CLUSTER`, `REASSIGN OWNED`, `IMPORT FOREIGN SCHEMA`, `SECURITY LABEL`, `DO` and
`REINDEX`, requiring all tokens to be consumed. `ANALYSE` and `MOVE` reuse the
parser's `ANALYZE` and `FETCH` grammars for classification. Missing arguments and
trailing tokens do not qualify. Compatibility forms outside this limited grammar
remain blocked with recoverable errors. DO bodies are opaque string literals:
recognizing the outer procedural command does not validate its language's body.
Import compatibility includes `LIMIT TO (...)` and `EXCEPT (...)` table filters
before `FROM SERVER`; each requires a non-empty, parenthesized identifier list.

The initial size and nested-comment checks remain recoverable. Tokenization or
parsing failure also rejects SQL without execution; the classifier does not
infer a serious operation from arbitrary words in malformed text. Unknown
commands are blocked with a recoverable error. Parser support is conservative:
valid PostgreSQL syntax outside the supported grammar may therefore be rejected.

## Enforcement and limits

The maintained command and function lists live in
[`sql_classification.rs`](../src/security/sql_classification.rs). They identify
known operations, not every possible behavior of database extensions or
user-defined functions. The classifier cannot infer function bodies or overload
semantics from a SQL call. Existing schema/relation policies, the verified
read-only database transaction, timeouts and result limits remain independent
defenses. Matching a known forbidden function name is deliberately conservative
across schemas and overloads.

Recoverable and serious rejections both prevent SQL execution. Only the serious
class produces `SafeselectError::Security`; the MCP handler audits the rejection,
responds with an error and exits with status 1. Internal validation and audit
failures retain their fatal behavior. See [the security model](security.md#6-fail-closed)
for response details and recovery instructions.

## Executable regression matrix

[`sql-classification.json`](../tests/fixtures/sql-classification.json) is the
case-by-case matrix, with `allowed`, `rejected` and `security` expectations.
It contains the original 37 review cases plus contextual and classification
boundaries. Both the security-engine unit test and the MCP process test consume
the same file. Extend it when adding a command class or correcting a boundary.

Additional classifier tests exercise every listed state-changing function in
scalar, table, lateral, CTE and EXPLAIN contexts, using ordinary, uppercase and
quoted qualified calls, with controls for similarly named identifiers and
functions. Existing tests cover configured schema policies and the
multi-statement toggle.

The process tests check continued tool discovery and `reconnect` availability
after ordinary errors, and exit status 1 after serious errors. Their backend is
intentionally unavailable: `allowed` proves entry into the backend path, not SQL
execution against PostgreSQL. Real-database checks follow the
[security suite contract](security-test-suite.md).
