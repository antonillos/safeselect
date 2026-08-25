# Security Proof

> Reviewed: 2026-08-25 · SafeSelect `v0.7.3`

This page describes what SafeSelect guarantees, what it only mitigates, and
what operators must still do. It does not replace native database permissions
or make an already-compromised connection safe.

## Verifiable summary

SafeSelect applies a read-only policy before executing an operation and keeps
multiple independent barriers:

1. Rust validates configuration, limits, policy, and the MCP request.
2. The Java sidecar validates the operations that reach the driver again.
3. The driver executes against PostgreSQL or MongoDB with time and result
   limits.
4. The database remains the final authority for identity and permissions.

Rust–Java communication uses JSON-lines over `stdin`/`stdout`; no socket or
HTTP port is opened.

![SafeSelect layers](safeselect-architecture.svg)

## Threat model

### Assets

- PostgreSQL and MongoDB data integrity.
- Credentials, DSNs, SSH keys, and local configuration.
- MCP and database process availability.
- Confidentiality of data, errors, and audit records.

### Included adversary

The MCP agent and every payload it can send are untrusted: SQL, BSON
filters/pipelines, JSON-RPC arguments, and repeated or oversized requests. The
workspace may also contain tampered configuration or a modified driver.

### Out of scope

- A host, process, or database account that is already compromised.
- A driver whose hash was deliberately changed in trusted configuration.
- Exfiltration from data to which the database role already has access.
- Operating-system, network, or database-provider denial of service.
- The semantic correctness of a legitimate query.

## Claims, evidence, and limits

| Claim | Type | Evidence | Limit |
|---|---|---|---|
| SQL does not allow DDL/DML or stacked queries | Guarantee | [`src/security.rs`](../src/security.rs), [`tests/security.rs`](../tests/security.rs), [`real_postgres.rs`](../tests/security_suite/real_postgres.rs) | Does not replace a database role without write permissions |
| CTEs, side-effecting functions, and `EXPLAIN ANALYZE` follow the read-only path | Guarantee | [`real_postgres.rs`](../tests/security_suite/real_postgres.rs) | A safe function can still consume resources; apply limits and database permissions |
| `$out`, `$merge`, `$where`, `$function`, and `$accumulator` are rejected | Guarantee | [`real_mongodb.rs`](../tests/security_suite/real_mongodb.rs), [`MainTest.java`](../sidecar/src/test/java/com/safeselect/MainTest.java) | Arbitrary MongoDB commands are not accepted |
| Schemas, relations, databases, and collections remain within policy | Guarantee | [`docs/security.md`](security.md) and security suites | The policy must be configured with least privilege |
| Results and execution are bounded | Mitigation | Limit tests in `real_postgres` and `real_mongodb` | Does not prevent all external resource consumption before the server responds |
| Secrets do not appear in project files, arguments, or audit records | Design guarantee | [`docs/security.md`](security.md), [`SECURITY.md`](../SECURITY.md) | The process memory and host still require protection |
| A security or audit failure terminates the process | Fail-closed guarantee | [`src/security.rs`](../src/security.rs), [`verify.yml`](../.github/workflows/verify.yml) | The operator must restart after correcting the cause |

## Attack → control → test matrix

| Attack | Control | Reproducible case |
|---|---|---|
| Stacked queries | Single-statement enforcement | `real_postgres` |
| CTE with DML | Read-only validation of the SQL tree | `real_postgres` |
| Side-effecting function | Validation plus a read-only database role | `real_postgres` |
| Transaction control | Rejection of transaction/session mutations | `real_postgres` |
| Unvalidated `EXPLAIN ANALYZE` | Explicit, validated execution path only | `real_postgres` |
| Deceptive comments and strings | Parser and structural validation | `tests/security.rs` |
| Denied schema/relation | Allowlist/denylist before driver access | `real_postgres` |
| Excessive rows, bytes, or execution time | Rust/sidecar/JDBC limits | `real_postgres`, `real_mongodb` |
| MongoDB `$out` / `$merge` | Aggregation-stage allowlist | `real_mongodb` |
| MongoDB server-side JavaScript | Recursive rejection in Rust and the sidecar | `real_mongodb`, `MainTest` |
| MongoDB filter flattening | Recursive validation | `real_mongodb` |
| Malformed or oversized JSON-RPC | Framing and size validation | `tests/mcp_negative.rs` |
| Secrets in driver errors | Redaction and bounded errors | Security suites |
| Audit failure | Mandatory initialization and termination | Audit tests |
| Retries/backpressure | Bounded retry and read limits | `real_mongodb`, smoke suite |

The names above refer to repository suites. Execution against real fixtures
requires the variables and services documented by the security workflow. A
suite that was not executed must not be presented as evidence.

## Deployment recommendation

Create a dedicated database role without `INSERT`, `UPDATE`, `DELETE`, DDL, or
administrative privileges; limit schemas and collections to what is required;
pin drivers by SHA-256; store secrets in Keychain or environment variables;
and review `audit_status`/`audit_recent`. SafeSelect adds controls, but defence
in depth also depends on those permissions and the host.

## CI and evidence updates

Automated evidence lives in [`verify.yml`](../.github/workflows/verify.yml) and
the suites under `tests/`. Update this page's version and links when controls
or cases change; a green badge is not a universal security proof.

```bash
cargo test --test security -- --nocapture
cargo test --test mcp_negative -- --nocapture
```

Suites using real databases must only run against explicitly identified,
disposable fixtures.

## Vulnerabilities

Report security issues privately according to [`SECURITY.md`](../SECURITY.md).
The public history will be updated when a vulnerability is confirmed, fixed,
and suitable for disclosure.
