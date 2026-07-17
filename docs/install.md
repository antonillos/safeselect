# Installation

## Prerequisites

- **Java 17+** (for the embedded database sidecar)
- **Rust 1.81+** (only if building from source)

The Java sidecar is embedded in the Rust binary, so you only need a Java 17+
runtime. No Maven or Rust is needed to run SafeSelect.

PostgreSQL environments also need a JDBC driver registered in the global config.
MongoDB support is included in the embedded sidecar and needs no separate driver.
The usual PostgreSQL setup path downloads its driver automatically during import,
or you can run:

```bash
safeselect driver download --vendor postgresql
```

## Homebrew (macOS)

```bash
brew install antonillos/tap/safeselect
```

The formula does not force-install a particular JDK. SafeSelect uses an existing
Java runtime when it is version 17 or newer and reports a clear error when Java
is missing or too old. If needed:

```bash
brew install openjdk@17
```

## asdf (macOS & Linux)

```bash
asdf plugin add safeselect https://github.com/antonillos/asdf-safeselect.git
asdf install safeselect latest
asdf set -u safeselect latest
asdf reshim safeselect latest
```

## From source

```bash
git clone https://github.com/antonillos/safeselect.git
cd safeselect

# Build the Java sidecar
cd sidecar
mvn package -DskipTests
cp target/safeselect-sidecar-*.jar target/safeselect-sidecar.jar
cd ..

# Build the Rust binary
cargo build --release

# The binary is at target/release/safeselect
./target/release/safeselect --version
```

## Quick install script

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

safeselect check --environment testing
safeselect agent install opencode --environment testing

# Later, after upgrading the safeselect binary:
safeselect agent upgrade opencode --environment testing
```

The agent installation writes an MCP stdio entry that runs `safeselect serve` for
one project and one environment. Agents do not receive raw database passwords.
`agent upgrade` also migrates older entry names to the canonical
`safeselect-<project>-<environment>` convention when it can detect the project.

During an interactive OpenCode installation, SafeSelect can use the existing
project-local config, create `.opencode/opencode.jsonc` alongside an existing
`.opencode/opencode.json`, or install to the global config.

For MongoDB Compass imports, SafeSelect also supports SSH-tunneled
`mongodb+srv://` connections. It resolves the SRV destination for the tunnel and
rewrites the local MongoDB endpoint with TLS, hostname-validation relaxation,
and direct-connection options required by the forwarded connection.

## Uninstall

```bash
safeselect uninstall
```

The uninstaller removes SafeSelect binaries installed under either `~/.local/bin`
or `~/.cargo/bin`, together with global config, data, audit logs, and Keychain entries.
