# asdf-safeselect

asdf plugin for [safeselect](https://github.com/antonillos/safeselect) — fail-closed read-only database access for AI agents over MCP.

## Install

```sh
asdf plugin add safeselect https://github.com/antonillos/asdf-safeselect.git
asdf install safeselect latest
SAFESELECT_VERSION="$(asdf latest safeselect | sed -n '$p')"
asdf set -u safeselect "${SAFESELECT_VERSION}"
```

## Requirements

- Java 17+
- A JDBC driver (install with `safeselect driver download --vendor postgresql`)

## MCP

safeselect works as an MCP server. After installation:

```sh
safeselect --help
```

## Release

This plugin is automatically updated by the safeselect release workflow.
