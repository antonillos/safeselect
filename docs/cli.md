# CLI reference

Use `safeselect --help` or `safeselect <command> --help` for help organized into
DESCRIPTION, USAGE, ARGUMENTS, FLAGS, GLOBAL FLAGS and SUBCOMMANDS as applicable.
Terminal help uses green flag names, lilac usage/subcommands and gray placeholders;
redirected output and `NO_COLOR=1` use plain text. Nested commands share the same
layout, including `safeselect help config set-password`.

| Command | Purpose |
|---|---|
| `safeselect serve [--environment <env>]` | Start the MCP server |
| `safeselect check [--environment <env>]` | Verify config, secrets, tunnels, sidecar, and backend connectivity for all environments by default |
| `safeselect doctor [--environment <env>]` | Print concise findings with stable codes for every environment by default |
| `safeselect posture [--environment <env>]` | Inspect PostgreSQL posture for every environment by default |
| `safeselect import-dbeaver <zip>` | Import DBeaver connections |
| `safeselect import-compose [--path <path>]` | Import from docker-compose |
| `safeselect import-compass [--path <path>]` | Import MongoDB Compass connections |
| `safeselect agent install <client> [--environment <env>]` | Install an MCP entry |
| `safeselect config set-password [--environment <env>]` | Configure the database password source |
| `safeselect config set-ssh-password [--environment <env>]` | Configure the SSH password source |
| `safeselect reconnect [--environment <env>]` | Verify connectivity using a temporary sidecar for each selected environment; all by default |
| `safeselect uninstall` | Remove installed binaries, global state, audit data, and Keychain entries |
| `safeselect uninstall --binary-only` | Remove only user-local binaries and preserve configuration |

See the [visual command gallery](cli-gallery.md) for examples and captures,
and [configuration](configuration.md) for environment selection and password sources.

## Connection diagnostics

Connection diagnostics distinguish SSH authentication, host-key, forwarding and
network failures from database authentication, driver, TLS, timeout and network
failures. JDBC startup errors retain a valid SQLSTATE (for example `28P01` for
authentication rejection) without printing exception details. An exited SSH
process stops tunnel polling promptly.

SafeSelect keeps the SSH process in the foreground while supervising tunnel
startup, overriding `ForkAfterAuthentication`. Released binaries extract the
embedded sidecar under a SHA-256-specific filename and verify its contents, so
updates use the matching sidecar without replacing a running older version.

If a database endpoint already responds, diagnostics report its reuse; that
response does not verify the remote destination behind an existing tunnel.
Multiple databases can share one bastion while keeping separate destinations,
local ports and database credentials. A warning identifies the selected bastion
when duplicate definitions of the same SSH host, port and username use different
credential references. Review those references and any environment override;
SafeSelect does not merge definitions or replace secrets automatically.

The interactive `config set-password` and `config set-ssh-password` prompts
reject empty input before completion and allow another attempt. The completion
label `[hidden]` does not indicate password length.

### Azure Bastion and existing tunnels

When using an Azure Bastion tunnel, open it before running SafeSelect and set
its SSH host and port to the local listener (for example `127.0.0.1:2222`).
The SSH connectivity diagnostic reports the configured endpoint and the actual
TCP connection or name resolution error; TCP success does not validate SSH
credentials. Run the Azure CLI tunnel in the same WSL distribution as SafeSelect,
or ensure that its Windows listener is accessible from WSL.
On SSH connectivity failure, SafeSelect also prints a safely quoted TCP probe
command for the configured endpoint, using Bash and `timeout` on Linux/WSL;
neither `nc` nor `telnet` is required. Run it from the same terminal environment
as SafeSelect. A successful TCP probe does not verify SSH or database authentication.
If an SSH tunnel cannot bind its local forwarding port, check for an existing
listener or stale SSH tunnel. A bastion restart can release an old SSH session
and its local listener, but TCP reachability alone cannot identify the cause.
MCP `connect` and `reconnect` report the recognized SSH failure category without
including endpoint or credential details.
When an existing database endpoint responds, `check` and `doctor` report its
reuse as information while still verifying the backend connection.
