---
name: safeselect-sql
description: >-
  Use for SafeSelect SQL schema discovery, bounded SELECT queries, PostgreSQL maintenance diagnostics, EXPLAIN plans, query timeouts and type or column errors. Discover exact table and column names before querying.
---

# SafeSelect SQL investigation

1. Call `database_info` if the backend is unknown.
2. Call `list_tables`. If the user only wants available tables, report and stop.
3. Choose one returned `table_schema` / `table_name` pair and call `describe_table`
   with those exact literal values. Never guess columns or pass wildcards.
4. Use only discovered names and types for a small targeted `select`. Narrow
   input in `WHERE` and add a small `LIMIT` for row retrieval.
5. For plans use `explain`, normally `format: "json"`, `analyze: false`.
   Do not send EXPLAIN through `select`. ANALYZE executes the query: use it only
   within an explicitly requested performance investigation and a bounded scope.
6. For PostgreSQL questions about tables needing ANALYZE or VACUUM, call
   `get_maintenance_diagnostics`. Report recommendations; never run maintenance.

Read [query recipes and recovery](references/workflows.md) for errors, JSON types
and performance. Follow server suggestions within read-only policy; never disable
validation or increase limits automatically.
