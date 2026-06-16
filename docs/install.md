# Installation

## Prerequisites

- **Java 17+** (for the JDBC sidecar)
- **Rust 1.81+** (only if building from source)

The Java sidecar is embedded in the Rust binary, so you only need the JDK at
runtime. No Maven or Rust is needed to run SafeSelect.

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
# safeselect 0.1.0
```
