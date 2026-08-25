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

## See it in action

The demos follow the same short, scenario-based format as makevn: each clip
has a focused story, a visible terminal recording, and the exact command below
it. Both agent tapes run with a temporary project-local MCP profile where
SafeSelect is enabled and the other MCP servers are disabled. Global agent
configuration is not modified.

```bash
./demo/setup.sh
safeselect agent install opencode --project "$PWD/demo" --environment postgres --local
```

### Agent discovery

![OpenCode agent discovering the database through SafeSelect MCP](recordings/safeselect-agent.gif)

```bash
opencode --pure run --dir demo 'Which three customers have the most recent paid orders? Give me their names and order totals.'
```

One business prompt. The agent chooses the discovery sequence and SafeSelect
returns the schema and bounded result without prior table knowledge.

### Read-only boundary

![OpenCode agent receiving a SafeSelect read-only rejection](recordings/safeselect-readonly.gif)

```bash
opencode --pure run --dir demo 'Remove every order that is not paid, and tell me what happened.'
```

The agent asks for the outcome; SafeSelect enforces the boundary and explains
the rejection.

### MongoDB agent

![OpenCode agent discovering MongoDB through SafeSelect MCP](recordings/safeselect-mongodb.gif)

```bash
opencode --pure run --dir demo 'Which security products are currently available? Give me their names, SKUs, and prices.'
```

The agent receives only the business question. It discovers the MongoDB document
structure and returns the matching synthetic products through SafeSelect.

The generated recordings are ignored by Git. Render any clip with:

```bash
vhs demo/safeselect-agent.tape
vhs demo/safeselect-readonly.tape
vhs demo/safeselect-mongodb.tape
```

Validate the versioned configuration and recording script without rendering:

```bash
./demo/validate.sh
```
