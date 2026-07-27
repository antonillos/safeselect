---
name: safeselect
description: >
  SafeSelect: fail-closed read-only database access for AI agents over MCP.
  Secure database access with read-only enforcement, AST-level SQL validation,
  and full fail-closed on any security incident.
tools:
  - database_info
  - select
  - list_tables
  - describe_table
  - explain
  - list_databases
  - list_collections
  - find_documents
  - aggregate_documents
  - distinct_documents
  - count_documents
  - explain_documents
  - profile_document_field
  - discover_document_schema
  - generate_document_fixture
  - connect
  - disconnect
  - reconnect
  - check
  - config_validate
  - config_show
  - config_set_password
  - config_rename_environment
  - config_delete_environment
  - config_reset
  - driver_list
  - driver_add
  - driver_download
  - agent_detect
  - agent_install
  - agent_uninstall
  - agent_status
  - import_compose
  - uninstall
setup: |
  # Install
  brew install antonillos/tap/safeselect
  # or via asdf:
  asdf plugin add safeselect https://github.com/antonillos/asdf-safeselect
  asdf install safeselect latest

  # Register a JDBC driver
  safeselect driver add --vendor postgresql --path /path/to/postgresql.jar --class org.postgresql.Driver

  # Install agent integration
  safeselect agent install opencode --project myproject --environment testing --name safeselect-myproject-testing

  # Upgrade from the current project and migrate to the default name
  safeselect agent upgrade opencode --environment testing

  # Import config from DBeaver export or docker-compose
  safeselect import-dbeaver ~/Downloads/dbeaver-export.zip
  safeselect import-compose --path compose.yml
  safeselect import-compass --path "$HOME/.config/MongoDB Compass"
commands:
  - safeselect serve --project <name> --environment <env>
  - safeselect config validate --project <name> --environment <env>
  - safeselect config show --project <name> --environment <env>
  - safeselect config rename-environment --old <name> --new <name>
  - safeselect config delete-environment --name <name>
  - safeselect check --project <name> --environment <env>
  - safeselect query --project <name> --environment <env> --sql "SELECT 1"
  - safeselect connect --project <name> --environment <env>
  - safeselect disconnect --project <name> --environment <env>
  - safeselect driver list
  - safeselect driver download --vendor postgresql
  - safeselect driver add --vendor postgresql --path <jar> --class <class>
  - safeselect agent detect
  - safeselect agent install <client> --project <p> --environment <e> --name <n>
  - safeselect agent upgrade <client> [--name <n>] [--project <p>] [--environment <e>]
  - safeselect agent uninstall <client> --name <n>
  - safeselect import-dbeaver <path-to-zip>
  - safeselect import-compose --path compose.yml
  - safeselect import-compass [--path <compass-file-or-directory>]
  - safeselect uninstall
config:
  directory: "~/.config/safeselect/"
  structure: |
    ~/.config/safeselect/          # global config
    ├── drivers/
    │   └── postgresql.toml
    └── sidecar/
        └── safeselect-sidecar.jar

    <repo-root>/.safeselect/       # per-project config
    ├── project.toml
    └── environments/
        ├── testing.toml
        └── production.toml
security:
  - Fail-closed: any security incident terminates the process
  - Read-only SQL validation for SELECT, EXPLAIN, and WITH
  - Fixed read-only MongoDB tools with database and collection policy enforcement
  - MongoDB aggregation rejects $out and $merge; counts require non-empty filters
  - Read-only enforcement per project policy
  - Secrets via macOS Keychain or env vars (never in config files)
  - Database passwords use sidecar stdin; SSH passwords use the sshpass environment, never process arguments
  - SHA-256 driver validation on every connection
  - No credentials in JDBC URLs
agent_guidance:
  - Use database_info before discovery when the backend is unknown
  - If the user only requested capabilities, tools, or available relations, report the discovery result and stop without reading data
  - For SQL data inspection, choose exactly one table_schema and table_name pair from list_tables, then call describe_table with those exact literal values; never pass placeholders, use wildcards, or guess column names
  - Use a small LIMIT for row retrieval; for COUNT or GROUP BY narrow input in WHERE because a final LIMIT does not reduce rows scanned
  - Use describe_table data_type and udt_name to choose type-compatible operators; PostgreSQL array udt_name values such as _jsonb identify the element type
  - Place SQL WITH CTEs at the beginning of the statement; do not nest WITH inside a subquery
  - After a missing-column error, call describe_table for every relation referenced by joins, unions, or subqueries, then use only returned column names and types
  - When GROUP BY rejects an aggregate expression or its ordinal position, group only by non-aggregate columns or omit GROUP BY for a single aggregate result
  - When PostgreSQL reports that an operator does not exist, rediscover types and use compatible operators; for JSON/JSONB use -> or ->> against observed fields and never cast blindly
  - For JSON/JSONB arrays such as udt_name _jsonb, use EXISTS with unnest and JSON operators on each observed element; never cast the array to text or use LIKE/ILIKE as a fallback
  - After a statement timeout, do not retry unchanged or broaden the query; preserve or narrow selective predicates and time bounds, avoid leading-wildcard LIKE or ILIKE on large relations, use a bounded discovery query then equality or IN, and never increase limits automatically
  - LIMIT helps row retrieval but does not by itself bound DISTINCT, GROUP BY, COUNT, or ORDER BY; after a timeout call the explain tool with analyze=false and never send EXPLAIN through select
  - For MongoDB, use list_databases, list_collections, then discover_document_schema before find or aggregate; never guess field names
  - Treat MongoDB schema discovery as sampled and non-exhaustive; an absent field may still exist outside the sample
  - Follow next_suggestion from discovery results and do not repeat an invalid query without rediscovering the target structure
  - Use bounded filters and limits for MongoDB analysis tools
  - Use explain with FORMAT JSON by default for agent parsing
  - Use explain analyze + buffers + explain_verbose for index and bottleneck analysis
  - Use format text only when the plan is meant for a human
  - Use check then reconnect to recover stale sidecar/JDBC/SSH tunnel failures
audit:
  - JSON audit log with query hashes (never full SQL)
  - Audit location: ~/.local/state/safeselect/audit/
