# LobeHub tool metadata

Regenerate the marketplace tool definitions when MCP names, descriptions or
schemas change. Requires Python 3.9+, a trusted SafeSelect executable matching
the manifest version, and an existing PostgreSQL JDBC JAR with an independently
verified SHA-256. Select a binary built from the code being documented: matching
version numbers alone cannot establish that the executable includes local changes.

```bash
python3 tools/distribution/update_lobehub_manifest.py \
  --binary /path/to/safeselect \
  --postgres-driver /path/to/postgresql.jar \
  --driver-sha256 <verified-sha256> \
  --check
```

Remove `--check` to update `lhm.plugin.json`, or use `--manifest` for a different
file. Exit codes: 0 = current/updated, 1 = drift in check mode, 2 = failure.
Review the diff before committing. Version, SEO description and all other
non-tool fields are preserved; Prepare Release owns the version bump.

## Determinism and idempotence

For the same metadata and MCP definitions, generation uses a stable tool-name
order, recursively sorted object keys, UTF-8 and LF output. Schema arrays retain
their original order because positional arrays can carry semantic meaning.
Temporary directory names are never included in the generated definitions.

When the definitions already match, update mode does not write the manifest:
both its bytes and modification time remain unchanged. `--check` never writes,
whether it succeeds or detects drift. Failed introspection leaves the file alone.
Updates are written to a temporary file in the manifest's directory, flushed and
synced, then atomically replaced while preserving file permissions. A partial
write or failed replacement leaves the original manifest intact.
Determinism assumes the trusted binary returns the same actual definitions; the
exporter does not hide changes in descriptions, schemas or array order.

The script uses temporary PostgreSQL/MongoDB project configurations, synthetic
credentials, isolated HOME/global configuration, and localhost port 1. It sends
only `initialize`, `notifications/initialized`, and `tools/list`. SafeSelect's
`initialize` handler currently pre-starts the sidecar, so each capture may launch
Java and attempt the configured localhost database handshake; failures are logged
by the server and the metadata response can still continue. No database queries
or MCP tool calls are made. The subprocess is bounded by a 20-second timeout.
This is not an OS sandbox for arbitrary executables: pass only a trusted
SafeSelect binary. Nothing is downloaded or published to LobeHub, and
authentication files are not read.

Backend-specific tools are labelled; `get_database_stats` preserves both backend
input schemas with `anyOf`. Unknown backend differences, pagination, duplicate
names, version mismatch and checksum mismatch fail rather than silently exporting
incomplete metadata. If MCP output or contextual descriptions change, review and
adapt the exporter. No resources or prompts are invented.

```bash
python3 -m unittest discover -s tools/distribution -p 'test_*.py'
```

After review, updating the existing marketplace listing remains a separate,
owner-authenticated `lhm plugin update --dir <absolute-repository-path>` step.
See the [official manifest reference](https://market.lobehub.com/s/publish-mcp/references/manifest).

## CI policy

Verify's Unit & Smoke job runs the exporter tests, builds the checked-out source
with `cargo build --locked`, and runs `--check` against that exact build. Its JDBC
download is pinned and SHA-256 verified. Runtime, manifest and exporter changes
select this job through the existing path policy; docs-only changes do not need
a new introspection. The required Verify gate propagates Unit & Smoke failures.

When CI detects drift, regenerate manually using a build of the changed source,
review the diff and include the JSON in the same PR. CI does not regenerate,
commit or publish metadata, and needs no LobeHub credentials. Release preparation
continues to synchronize the version separately.

## Local manifest contract validation

Both modes validate the saved manifest before introspection and the proposed
result before writing. Checks cover required strings, identifier syntax, known
top-level fields, URL fields, tags, capability arrays, tool names/object-schema
shapes and localization field types. Duplicate JSON keys, non-JSON numeric
constants and duplicate tool names/locales are rejected. Validation errors return
2 with fixed field-level messages, without printing supplied values.

These are local checks based on the official reference reviewed on 2026-08-28,
**not an official LobeHub JSON Schema**. As a stricter local policy, unknown
top-level fields are rejected instead of silently discarded; URL fields require
HTTP(S) and cannot contain credentials. Tool extension fields are preserved.
This is not a full JSON Schema metaschema validator for nested input/output
schemas. Marketplace category/locale availability, first-publication requirements
(including description/new translations), ownership and remote acceptance still
require LobeHub validation. No remote schemas or URLs are fetched by these checks.
