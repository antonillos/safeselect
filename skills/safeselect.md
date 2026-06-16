---
name: safeselect
description: >
  SafeSelect: MCP SQL Fail-Closed for AI Agents.
  Secure database access with read-only enforcement, AST-level SQL validation,
  and full fail-closed on any security incident.
tools:
  - select
  - list_tables
  - explain
setup: |
  # Install
  brew install anomalyco/tap/safeselect
  # or via asdf:
  asdf plugin add safeselect https://github.com/anomalyco/asdf-safeselect
  asdf install safeselect latest

  # Register a JDBC driver
  safeselect driver add --vendor postgresql --path /path/to/postgresql.jar --class org.postgresql.Driver

  # Install agent integration
  safeselect agent install opencode --project myproject --environment testing --name myproject-testing

  # Generate config from DBeaver export
  safeselect import dbeaver ~/Downloads/dbeaver-export.zip
commands:
  - safeselect serve --project <name> --environment <env>
  - safeselect config validate --project <name> --environment <env>
  - safeselect config show --project <name> --environment <env>
  - safeselect check --project <name> --environment <env>
  - safeselect driver list
  - safeselect driver download --vendor postgresql
  - safeselect driver add --vendor postgresql --path <jar> --class <class>
  - safeselect agent detect
  - safeselect agent install <client> --project <p> --environment <e> --name <n>
  - safeselect agent uninstall <client> --name <n>
  - safeselect import dbeaver <path-to-zip>
config:
  directory: "~/.config/safeselect/"
  structure: |
    ~/.config/safeselect/
    ├── drivers/
    │   └── postgresql.toml
    └── projects/
        └── <project>/
            ├── project.toml
            └── environments/
                ├── testing.toml
                └── production.toml
security:
  - Fail-closed: any security incident terminates the process
  - AST-level SQL validation (PostgreSQL parser)
  - Read-only enforcement per project policy
  - Secrets via macOS Keychain or env vars (never in config files)
  - SHA-256 driver validation on every connection
  - No credentials in JDBC URLs
audit:
  - JSON audit log with query hashes (never full SQL)
  - Audit location: ~/.local/state/safeselect/audit/
