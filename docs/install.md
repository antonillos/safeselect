# Installation

## Prerequisites

- **Java 17+** (for the JDBC sidecar)
- **Rust 1.81+** (only if building from source)

The Java sidecar is embedded in the Rust binary, so you only need the JDK at
runtime. No Maven or Rust is needed to run SafeSelect.

SafeSelect also needs a PostgreSQL JDBC driver registered in its global config.
The usual setup path downloads it automatically during import, or you can run:

```bash
safeselect driver download --vendor postgresql
```

## Homebrew (macOS)

```bash
brew install antonillos/tap/safeselect
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
# safeselect 0.3.0
```

## First Project Setup

For most users, import existing connection details and then install the MCP entry
for the agent:

```bash
safeselect import-dbeaver ~/Downloads/dbeaver-export.zip
# or:
safeselect import-compose

safeselect check --environment testing
safeselect agent install opencode --environment testing
```

The agent installation writes an MCP stdio entry that runs `safeselect serve` for
one project and one environment. Agents do not receive raw database passwords.
