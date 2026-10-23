# SQL recipes and recovery

`LIMIT` bounds retrieved rows, not work performed by DISTINCT, COUNT, GROUP BY
or ORDER BY. Narrow input predicates and time ranges first. After a timeout,
never retry unchanged, remove selective filters, or add leading-wildcard
LIKE/ILIKE on a large relation. Use `explain` with `analyze=false` and revise the
query within the original constraints. Ask before increasing configured limits.

After a missing-column error, describe every relation used by joins, unions or
subqueries and use only returned columns. After an operator/type error, inspect
`data_type` and `udt_name` again. Use type-compatible operators; never cast blindly.
PostgreSQL array UDT names such as `_jsonb` identify the element type. For JSONB
arrays use EXISTS with unnest and JSON operators on observed elements; never
cast the array to text or fall back to LIKE/ILIKE. For JSON/JSONB values use
`->` or `->>` against observed fields.

Place WITH CTEs at the start of the statement, not nested in a subquery. If GROUP
BY rejects an aggregate or its ordinal position, group only by non-aggregate
columns or omit GROUP BY for a single aggregate result.

For index and bottleneck analysis, first inspect a non-executing JSON plan.
Within an authorized bounded performance investigation, `analyze`, `buffers`
and `explain_verbose` can be requested together. Use text format when the plan
is intended for a human. Neither a skill nor a query error authorizes writes.

CLI equivalents include `safeselect query --environment <env> --sql <sql>`;
use CLI `--help` and actual MCP schemas rather than inventing equivalent flags.
CLI execution uses a separate session; use MCP for the active agent session.
