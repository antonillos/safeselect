---
name: safeselect-mongodb
description: >-
  Use for SafeSelect MongoDB database and collection discovery, sampled schemas, bounded document reads and aggregation, indexes, statistics, query explanations or redacted fixtures. Preserve complete nested filters and database constraints.
---

# SafeSelect MongoDB investigation

1. Call `database_info` if the backend is unknown, then `list_databases` and
   `list_collections` using exact returned names. Stop here for listing requests.
2. Call `discover_document_schema` before reading unfamiliar documents. Schema
   discovery is sampled, not exhaustive; missing fields may exist elsewhere.
3. Use observed fields in bounded `find_documents` or `aggregate_documents` calls.
   Preserve database/collection policy, filters, projection and small limits.
4. Pass filter/projection/sort as complete nested JSON objects and pipeline as
   one complete array. If a client flattens values, use a JSON-encoded object
   or array string. Never send keys like `filter.name` or `pipeline[0].$match`.
5. After flattened or missing-filter rejection, preserve the intended constraint
   in a complete value. Never substitute `{}` or retry the same malformed call.
6. Follow the returned next step once; report terminal failures and stop.

Read [document tools and recovery](references/workflows.md) for analysis, fixtures,
indexes and statistics. Never bypass SafeSelect policy using raw MongoDB access.
