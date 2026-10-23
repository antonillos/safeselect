# Document tools and recovery

`find_documents` requires a filter. `count_documents` requires a non-empty filter;
never replace it with `{}`. Use `distinct_documents` for a bounded field summary,
`profile_document_field` for one observed path and `explain_documents` for a
bounded find plan. Sampled schema is not proof of field absence.

`aggregate_documents` requires a non-empty array of JSON-object stages. `$out`
and `$merge` are writes and rejected. `$where`, `$function` and `$accumulator`
are rejected at any depth. Preserve scope and rebuild with declarative MQL;
retry once only after correction. Never request enabled server-side JavaScript
or relaxed policy. A terminated MCP session requires host recovery.

`list_collection_indexes` returns classic index metadata and, when permitted,
Atlas Search/Vector metadata. If `search_indexes_status` is `unsupported` or
`unauthorized`, use classic indexes and do not retry the Search request.
`get_database_stats` and `get_collection_stats` return bounded storage counters,
not raw stats commands or documents. Follow their discovery/index next steps.

`generate_document_fixture` returns anonymized samples, never writes a file.
Pass `redact_fields` as a complete string array or JSON-encoded string array;
never flatten, omit intended redactions or publish unreviewed private samples.
All object/array string fallbacks use the same strict validation and read policy.

After field/path errors, rediscover the collection before correcting the request.
After timeouts, preserve or narrow filters, sample sizes and limits. Do not
broaden the query, increase project limits or repeat it unchanged.
