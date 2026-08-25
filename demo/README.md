# SafeSelect demo fixtures

This directory provides synthetic PostgreSQL and MongoDB data for the SafeSelect
demo and its VHS recording. It contains no real credentials or personal data.

## Start

```bash
./demo/setup.sh
```

The isolated stack uses ports `55432` (PostgreSQL) and `57017` (MongoDB) so it
does not take over the default development ports.

## Dataset

- PostgreSQL: 12 customers, 12 products, 24 orders, order items and 24 events.
- MongoDB: 18 customers, 24 products, 36 orders and 48 events.
- Both backends include nested JSON, arrays, timestamps, booleans, numeric
  values, nullable fields, indexes, statuses, categories and relationships.
- Every value is deterministic and uses `example.test`, TEST-NET addresses and
  synthetic names.
- The Compose file pins the tested multi-platform image digests; update them
  deliberately when refreshing the fixture environment.

`reset` removes the named Docker volumes before recreating the fixture. Use
`stop` when you want to preserve the data between runs.

## Agent demo

The checked-in `.safeselect/` project has `postgres` and `mongodb`
environments. To prepare the real OpenCode agent demo, run:

```bash
./demo/setup.sh
safeselect agent install opencode --project "$PWD/demo" --environment postgres --local
```

This only registers SafeSelect as the project's MCP server; it does not tell
the agent which tools, tables, columns, data types, or indexes to use.
`demo/setup.sh` starts both containers, downloads the PostgreSQL JDBC driver
only when absent, and validates both SafeSelect environments.

## VHS

The primary 30–45 second marketing recording is versioned as
`demo/safeselect-demo.tape`. Run the setup once, then render it with:

```bash
vhs demo/safeselect-demo.tape
```

The generated recording is ignored by Git. It is a real OpenCode session: the
agent receives one natural-language request and independently discovers the
schema and performs its bounded read through SafeSelect MCP. The tape contains
no prescribed tool calls, tables, columns, data types, or indexes.

Validate the versioned configuration and recording script without rendering:

```bash
./demo/validate.sh
```
