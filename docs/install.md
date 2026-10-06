# Installation

## Prerequisites

- **Java 17+** (for the embedded database sidecar)
- **Rust 1.85+** (only if building from source)

The Java sidecar is embedded in the Rust binary, so you only need a Java 17+
runtime. No Maven or Rust is needed to run SafeSelect.

PostgreSQL environments also need a JDBC driver registered in the global config.
MongoDB support is included in the embedded sidecar and needs no separate driver.
The usual PostgreSQL setup path downloads its driver automatically during import,
or you can run:

```bash
safeselect driver download --vendor postgresql
```

## Package managers

The prebuilt installer, Homebrew, and asdf provide the recommended
easy-installation methods. Choose one for your platform, then continue with the
common setup below.

### Homebrew (macOS)

```bash
brew install antonillos/tap/safeselect
```

The formula does not force-install a particular JDK. SafeSelect uses an existing
Java runtime when it is version 17 or newer and reports a clear error when Java
is missing or too old. If needed:

```bash
brew install openjdk@17
```

### asdf (macOS & Linux)

```bash
asdf plugin add safeselect https://github.com/antonillos/asdf-safeselect.git
asdf install safeselect latest
SAFESELECT_VERSION="$(asdf latest safeselect | sed -n '$p')"
asdf set -u safeselect "${SAFESELECT_VERSION}"
asdf reshim safeselect "${SAFESELECT_VERSION}"
```

## From source

```bash
git clone https://github.com/antonillos/safeselect.git
cd safeselect
./install.sh
"$HOME/.local/bin/safeselect" --version
```

The script builds the Java sidecar and Rust release binary, then installs
`safeselect` under `~/.local/bin` by default. Use `PREFIX` or `BIN_DIR` to
select a different destination. Requirements: Rust 1.85+, Java 17+, and
`makevn`. If makevn is missing and you use Homebrew or asdf, opt in to its
installation with `./install.sh --install-makevn`. Add `~/.local/bin` to your
`PATH` before invoking `safeselect` without its full path.

## Prebuilt binary installer

For a platform-specific prebuilt binary on macOS or glibc-based Linux, see the
[latest GitHub release](https://github.com/antonillos/safeselect/releases/latest)
or run the verified installer. It checks the release archive's SHA-256 digest
before installing to `~/.local/bin` (override with `PREFIX`):

```bash
curl -fsSL https://raw.githubusercontent.com/antonillos/safeselect/main/packaging/install/install-release.sh | sh
```

## Verify installation

```bash
safeselect --version
# safeselect <version>
```

## First Project Setup

For most users, import existing connection details and then install the MCP entry
for the agent:

```bash
safeselect import-dbeaver ~/Downloads/dbeaver-export.zip
# or:
safeselect import-compose
# or:
safeselect import-compass --path "$HOME/.config/MongoDB Compass"

# Check all environments; add --environment <name> to restrict the target.
safeselect check
safeselect agent install opencode

# Later, after upgrading the safeselect binary:
safeselect agent upgrade opencode
```

The agent installation writes an MCP stdio entry that runs `safeselect serve` for
one project and one environment. Agents do not receive raw database passwords.
`agent upgrade` also migrates older entry names to the canonical
`safeselect-<project>-<environment>` convention when it can detect the project.

SafeSelect follows the repository convention before requiring flags: it finds
the nearest `.safeselect/` directory and uses its sole environment. Supply
`--project` outside that repository or `--environment <name>` when the project
contains more than one environment; single-environment commands never guess
among several candidates.

Unlike single-environment commands, `check`, `doctor`, `posture`,
`reconnect`, and `config validate` process all environments by default. Checks
can access secrets, open tunnels, and connect to databases. Select an environment
explicitly when other connections, especially production, must remain untouched.
See [CLI conventions and effects](../README.md#convention-before-configuration).

During an interactive OpenCode installation, SafeSelect can use the existing
project-local config, create `.opencode/opencode.jsonc` alongside an existing
`.opencode/opencode.json`, or install to the global config.

For MongoDB Compass imports, SafeSelect also supports SSH-tunneled
`mongodb+srv://` connections. It resolves the SRV destination for the tunnel and
rewrites the local MongoDB endpoint with TLS, hostname-validation relaxation,
and direct-connection options required by the forwarded connection.

## Password environment references

Configure either password using a whole-input `{env:NAME}` reference, on macOS,
Linux or WSL. SafeSelect saves the reference without reading or storing its value:

```bash
safeselect config set-password --environment staging --password '{env:DB_PASSWORD}'
safeselect config set-ssh-password --environment staging --password '{env:BASTION_PASSWORD}'
read -rsp 'Database password: ' DB_PASSWORD; echo; export DB_PASSWORD
read -rsp 'Bastion password: ' BASTION_PASSWORD; echo; export BASTION_PASSWORD
safeselect check --environment staging
```

The input commands above target Bash. Export the variables in the shell that
launches SafeSelect or the MCP client. Missing or empty required variables fail
closed. SSH password authentication requires `sshpass`. `{file:...}` and general
configuration interpolation are not supported. On macOS, ordinary password input
still uses Keychain; `--literal-password` opens a secure literal prompt even for
passwords that resemble references. Never put real literal passwords in command
arguments. See [password sources](../README.md#password-references-on-all-platforms).

## Uninstall

```bash
safeselect uninstall
```

The uninstaller removes SafeSelect binaries installed under either `~/.local/bin`
or `~/.cargo/bin`, together with global config, data, audit logs, and Keychain entries.

To remove only a locally installed development binary before switching to the
Homebrew release, use:

```bash
safeselect uninstall --binary-only
```

This preserves global and project configuration, database drivers, audit data,
and Keychain entries.
