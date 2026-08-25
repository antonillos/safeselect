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

## SafeSelect environments

The checked-in `.safeselect/` project has `postgres` and `mongodb`
environments. Source the demo environment before using it:

```bash
source demo/env.sh
./demo/show-postgres.sh
./demo/show-mongodb.sh
```

`demo/setup.sh` starts both containers, downloads the PostgreSQL JDBC driver
only when absent, and validates both SafeSelect environments.

## VHS

The primary 30–45 second marketing recording is versioned as
`demo/safeselect-demo.tape`. Run the setup once, then render it with:

```bash
vhs demo/safeselect-demo.tape
```

The generated recording is ignored by Git. Its visible actor is an AI agent:
it receives natural-language requests, discovers and reads through SafeSelect
MCP tools, then sends a mutation request that SafeSelect rejects fail-closed.
The scripts are deterministic MCP-client harnesses for recording; they do not
pretend that a live model produced the transcript.

Validate the versioned configuration and recording script without rendering:

```bash
./demo/validate.sh
```
