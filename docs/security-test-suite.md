# Database MCP Read-Only Security Test Suite

This suite turns SafeSelect's security claims into reproducible regression
tests against disposable PostgreSQL and MongoDB fixtures. It is intentionally
separate from unit tests: every case records a baseline and verifies that a
rejected operation did not mutate the fixture.

## Safety contract

- Never point the suite at a non-disposable database.
- Fixtures use isolated test databases and synthetic data.
- Destructive payloads are expected, but only inside the identified fixtures.
- A case is passing only when the request is rejected for the expected reason
  and the final database state equals the baseline.
- A skipped integration suite is not evidence of a passing security claim.

## Current adapters

| Adapter | Entry point | Coverage |
|---|---|---|
| PostgreSQL | [`real_postgres.rs`](../tests/security_suite/real_postgres.rs) | Read-only SQL, stacked statements, CTE DML, transaction control, side-effecting functions, schema policy, result limits, and timeouts |
| MongoDB | [`real_mongodb.rs`](../tests/security_suite/real_mongodb.rs) | Database/collection policy, nested namespace policy, recursive pipeline stages, `$out`/`$merge`, metadata stages, bounded resource options, server-side JavaScript, result limits, timeouts, and retry guidance |
| MCP protocol | [`mcp_negative.rs`](../tests/mcp_negative.rs) | Malformed JSON-RPC, oversized payloads, and fail-closed process behaviour |

## Running locally

Build the sidecar and fixtures using the repository workflow before running
real database tests:

```bash
makevn doctor init test package
cp sidecar/target/safeselect-sidecar-*.jar sidecar/target/safeselect-sidecar.jar

SAFESELECT_SECURITY_TEST=1 \\
  cargo test --test security real_postgres_security_rejections_and_limits -- --nocapture

SAFESELECT_SECURITY_TEST=1 \\
  cargo test --test security real_mongodb_security_rejections_and_limits -- --nocapture
```

The CI workflow supplies the fixture services and credentials through
ephemeral job-scoped environment variables. See
[`.github/workflows/verify.yml`](../.github/workflows/verify.yml) for the
canonical MongoDB invocation and
[`integration-tests.yml`](../.github/workflows/integration-tests.yml) for the
full fixture workflow.

## Versioned case corpus

The initial case corpus is stored in
[`tools/security/adversarial-cases.json`](../tools/security/adversarial-cases.json).
It is backend-neutral: adapters map each case to their fixture and preserve the
expected decision and state-invariance contract.

The corpus currently contains 45 cases: 35 implemented against the existing
regression adapters and 10 planned cases covering stored-procedure policy,
parser allow cases, dynamic SQL, and MCP framing. Planned cases remain
visible in the manifest but must not be reported as executed evidence until an
adapter consumes them.

Validate it with the standard-library reporter:

```bash
tools/security/validate_manifest.py
tools/security/validate_manifest.py --json
```

## Adding a case

1. Add a stable case to the versioned JSON corpus.
2. Add the payload to the relevant backend adapter.
3. Capture the fixture baseline before the case group.
4. Assert the expected rejection category, not only a generic process error.
5. Assert that the final state equals the baseline.
6. Add the case to the attack → control → test table in
   [`security-proof.md`](security-proof.md).
7. Keep the case deterministic and safe to rerun.

The next extraction step is to execute the remaining PostgreSQL side-effect
cases and MongoDB resource-option cases, so other MCP implementations can reuse
the same attack corpus without importing SafeSelect's backend harness.

## Security references

- [PostgreSQL read-only transaction restrictions](https://www.postgresql.org/docs/current/sql-set-transaction.html)
- [PostgreSQL sequence state changes](https://www.postgresql.org/docs/17/functions-sequence.html)
- [PostgreSQL function security](https://www.postgresql.org/docs/17/perm-functions.html)
- [MongoDB aggregation stages](https://www.mongodb.com/docs/current/reference/operator/aggregation-pipeline/)
- [MongoDB aggregation resource limits](https://www.mongodb.com/docs/manual/reference/command/aggregate/)
- [OWASP SQL Injection Prevention Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/SQL_Injection_Prevention_Cheat_Sheet.html)
