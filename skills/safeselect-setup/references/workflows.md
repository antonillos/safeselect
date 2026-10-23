# Setup recipes and effects

## Application setup

Install SafeSelect using `brew install antonillos/tap/safeselect`, or the documented
release/source distribution. Run setup from the application repository.
Import a requested connection with `safeselect import-dbeaver <export.zip>`,
`safeselect import-compose`, or `safeselect import-compass --path <source>`.
Imports create project configuration; inspect redacted results before connecting.
Do not invent private paths or connection values.

CLI commands accepting one environment infer it only when unambiguous. `check`,
`doctor`, `posture`, `reconnect` and `config validate` process all environments
by default; restrict with `--environment` when appropriate. Checks can open SSH
tunnels and database connections. Use each command's `--help` for accepted flags.

## Agent registration

`safeselect agent detect` identifies clients. After project setup:

```sh
safeselect agent install codex --project <application-repository>
safeselect agent install opencode --project <application-repository> --local
safeselect agent upgrade codex --project <application-repository>
safeselect agent status
```

Install and upgrade register MCP and four skills for Codex and OpenCode.
Global skills live in `~/.agents/skills`; `--local` uses `.agents/skills` in the
resolved application project. Skills are shared across entries and these clients.
Re-register after upgrading the binary. Status checks files and configuration;
it does not prove discovery or loading. Reload the client, inspect its skills
catalog and explicitly load the skills before evaluating automatic selection.
Never change client trust or skill permissions to force discovery.

`agent uninstall <client> --name <entry>` removes MCP only and retains shared
skills. Edited or foreign skill content blocks updates: preserve it and report
conflicting paths; never delete it or bypass ownership verification automatically.

## MCP equivalents

Use `import_compose`, `config_validate`, `config_show`, `driver_list`,
`driver_download`, `driver_add`, `config_set_password`, `check`, `reconnect`,
`agent_detect`, `agent_install` and `agent_status` according to advertised schemas.
`agent_install` supports `local`; it writes integration, so discovery alone is
not authorization to call it. CLI `doctor` and `posture` are diagnostics with
no assumed same-named MCP equivalent.
