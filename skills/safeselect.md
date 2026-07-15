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
  - SHA-256 driver validation on every connection
  - No credentials in JDBC URLs
agent_guidance:
  - Use list_tables before guessing schema names
  - Use database_info before discovery when the backend is unknown
  - Use list_databases and list_collections before MongoDB reads
  - Use bounded filters and limits for MongoDB analysis tools
  - Use explain with FORMAT JSON by default for agent parsing
  - Use explain analyze + buffers + explain_verbose for index and bottleneck analysis
  - Use format text only when the plan is meant for a human
  - Use check then reconnect to recover stale sidecar/JDBC/SSH tunnel failures
audit:
  - JSON audit log with query hashes (never full SQL)
  - Audit location: ~/.local/state/safeselect/audit/
