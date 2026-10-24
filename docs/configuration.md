# Configuration

Global state lives in `~/.config/safeselect/` by default. Project policy lives in `.safeselect/` at the repository root:

```text
<repo-root>/
└── .safeselect/
    ├── project.toml
    └── environments/
        └── <env>.toml
```

SafeSelect walks upward from the current directory to find `.safeselect/`. Use `--project <path>` when an agent or script should target a specific repository.

## Convention before configuration

From inside a configured repository, commands infer the project from the nearest
`.safeselect/` directory and infer the environment when exactly one
`environments/*.toml` file exists. For example, `safeselect serve`,
`safeselect check`, `safeselect query --sql "SELECT 1"`, and
`safeselect config show` need no project or environment flags in a
single-environment project. `--project` and `--environment` remain available
for scripts, other working directories, and deliberate selection. If multiple
environments exist, commands that act on one fail rather than guess and tell
you to pass `--environment <name>`.

The defaults differ by operation:

- `serve`, `query`, `connect`, `disconnect`, `config show`, and password
  commands require one explicit or uniquely inferred environment. With no
  environments they fail; `serve` has the separate [first-run setup behavior](agents.md#first-run-setup).
- `check`, `doctor`, `posture`, `reconnect`, and `config validate` inspect or
  process all environments when the flag is omitted. Checks are not offline:
  they can resolve secrets, establish SSH tunnels, and contact databases. Use
  `--environment <name>` to avoid touching unrelated or production environments.
- `query` supports JDBC environments only and still needs SQL through `--sql`
  or stdin (interactive stdin waits for EOF). Selecting a MongoDB environment
  does not turn it into a MongoDB query command.
- CLI `connect` and `disconnect` operate on a new, temporary JDBC sidecar and
  shut it down before exiting. They do not control an already-running MCP
  session; use that session's MCP connection tools instead. CLI `reconnect`
  likewise creates and closes its own sidecar, and supports both PostgreSQL and
  MongoDB.
- Password commands modify local Keychain/configuration, not the database
  password itself. `config set-ssh-password` switches SSH authentication to
  password and clears the configured identity-file reference.

## Password references on all platforms

Use OpenCode-style **`{env:NAME}`** references for database and SSH passwords on
macOS, Linux and WSL. These commands save only the variable name; they do not
read its value or store the password:

```bash
safeselect config set-password --environment staging --password '{env:DB_PASSWORD}'
safeselect config set-ssh-password --environment staging --password '{env:BASTION_PASSWORD}'
```

Enter the values without putting passwords in shell history (Bash):

```bash
read -rsp 'Database password: ' DB_PASSWORD; echo; export DB_PASSWORD
read -rsp 'Bastion password: ' BASTION_PASSWORD; echo; export BASTION_PASSWORD
safeselect check --environment staging
```

The variables must be exported in the process that launches SafeSelect or the
MCP client. A missing or empty required variable fails closed. No Python helper,
`.env` auto-loading, shell evaluation, `{file:...}` reading or general TOML
interpolation is performed. Password-based SSH tunnels still require `sshpass` and do not fall back to
SSH-agent/key authentication.
The saved configuration uses the existing `database.secret.source = "env"`
and `variable` fields, and SSH `secret_variable`; no new password field is needed.

On macOS, the `config set-password` and `config set-ssh-password` prompts also
accept `{env:NAME}`; literal passwords use Keychain. Importers offer a separate
password-source choice; their environment-reference prompts accept either `NAME`
or `{env:NAME}`. Use `config set-password` after import to select a short database
variable name instead of the generated default. `config_set_password` through
MCP accepts the same reference; prefer references rather than sending raw
passwords through an agent.

References must occupy the entire input and use a valid shell variable name.
Imported credentials are opaque passwords, not expressions. If a literal macOS
password happens to look like a reference, use the secure literal prompt:

```bash
safeselect config set-password --environment staging --literal-password
# or: safeselect config set-ssh-password --environment staging --literal-password
```

Avoid supplying real literal passwords with `--password`: command arguments can
appear in shell history and process listings. Existing Keychain and environment
references remain valid. Custom variable names are preserved when renaming an
environment; changing a source does not delete the old Keychain entry.

## SSH passwords on Linux and WSL

Interactive Compass and DBeaver imports let you reference an exported variable,
enter a password with hidden input, or use a password from the export. On
Linux/WSL, literal passwords are held only for that import process. The saved
configuration contains the variable name, and later commands need its value
exported again by their launching shell. On macOS, the importer also offers
persistent Keychain storage.

The interactive import defaults are names such as `MYAPP_STAGING_DB_PASSWORD`
and `STAGING_SSH_PASSWORD`. Review them before saving: normalized names can
collide, and SSH defaults can be shared across projects. Existing references are
preserved unless explicitly changed. Other setup paths may generate names with
a `SAFESELECT_PASSWORD_` or `SAFESELECT_SSH_PASSWORD_` prefix; use the exact name
printed by the command or saved in the configuration.

An SSH environment reference is stored as `secret_variable`, never as a
plaintext password:

```toml
[ssh]
enabled = true
auth_type = "PASSWORD"
secret_variable = "SAFESELECT_SSH_PASSWORD_DEMO"
```

Export the password in the shell that launches SafeSelect or the MCP client.
For Bash in WSL, this avoids putting the password in shell history:

```bash
read -rsp 'SSH password: ' SAFESELECT_SSH_PASSWORD_DEMO; echo
export SAFESELECT_SSH_PASSWORD_DEMO
```

Password-based tunnels also require `sshpass`. Missing or empty secrets fail
closed; SafeSelect does not fall back to a different secret source. Do not set
both `secret_account` (macOS Keychain) and `secret_variable`.
For an existing Linux/WSL environment, run `safeselect config set-ssh-password
--environment <env>` without `--password` to configure an environment reference,
then export the variable it prints. The command cannot export into its parent
shell. Restart a running MCP client after changing its environment.

Imports defer verification when a required database or SSH variable is unset or
empty. Export the variable before checking the connection. A password supplied
for one import session does not persist into later CLI commands or MCP clients.
MongoDB imports remove literal credentials from the saved URI.

Generated MCP entries deliberately pin both project and environment. Keep those
arguments in client configuration and unattended scripts: inference is a CLI
convenience, not a persistent default environment.
