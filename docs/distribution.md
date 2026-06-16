# Distribution

SafeSelect is distributed as a single Rust binary with an embedded Java sidecar
JAR. No installation of the sidecar is needed — it is extracted at runtime.

## Release Process

1. Bump version in `Cargo.toml` and `sidecar/pom.xml`
2. Push a tag `v<semver>` (e.g., `v0.1.0`)
3. GitHub Actions builds for 4 targets:
   - `aarch64-apple-darwin`
   - `x86_64-apple-darwin`
   - `aarch64-unknown-linux-gnu`
   - `x86_64-unknown-linux-gnu`
4. Each release includes the binary + SHA-256 checksum
5. Homebrew formula is updated in `antonillos/homebrew-tap`
6. asdf plugin is updated in `antonillos/asdf-safeselect`

## Package Managers

### Homebrew

```bash
brew install antonillos/tap/safeselect
```

The formula is at [github.com/antonillos/homebrew-tap](https://github.com/antonillos/homebrew-tap).

### asdf

```bash
asdf plugin add safeselect https://github.com/antonillos/asdf-safeselect.git
```

The plugin is at [github.com/antonillos/asdf-safeselect](https://github.com/antonillos/asdf-safeselect).

## Release Assets

Each GitHub release contains:

- `safeselect-<target>.tar.gz` — compiled binary + embedded sidecar
- `safeselect-<target>.tar.gz.sha256` — checksum

## Binary Contents

The `safeselect` binary includes:
- Rust CLI + MCP server
- Embedded `safeselect-sidecar.jar` (2.2 MB with Jackson)

**Not included**: JDBC drivers. Install them separately via `safeselect driver download --vendor postgresql`.
