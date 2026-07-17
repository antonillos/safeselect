# Distribution

SafeSelect is distributed as a single Rust binary with an embedded Java sidecar
JAR. No installation of the sidecar is needed — it is extracted at runtime.

## Release Process

1. Run the **Prepare Release** workflow against the intended base branch; it bumps
   `Cargo.toml`, `Cargo.lock`, and `sidecar/pom.xml`, updates `CHANGELOG.md`, and opens a release PR.
2. Merge the release PR into `main`. A version change on `main` starts the release workflow;
   it can also be dispatched manually with an explicit tag and target ref.
3. Integration tests must pass before the GitHub release and assets are published.
4. GitHub Actions builds for 4 targets:
   - `aarch64-apple-darwin`
   - `x86_64-apple-darwin`
   - `aarch64-unknown-linux-gnu`
   - `x86_64-unknown-linux-gnu`
5. Each release includes the binary plus a SHA-256 checksum.
6. Non-draft, non-prerelease releases update Homebrew and asdf when the release token is configured.

## Package Managers

### Homebrew

```bash
brew install antonillos/tap/safeselect
```

The formula is at [github.com/antonillos/homebrew-tap](https://github.com/antonillos/homebrew-tap).
It intentionally does not depend on Homebrew's `openjdk@17` formula: SafeSelect
accepts any available Java 17+ runtime and reports how to install one when no
compatible runtime is found.

### asdf

```bash
asdf plugin add safeselect https://github.com/antonillos/asdf-safeselect.git
```

The plugin is at [github.com/antonillos/asdf-safeselect](https://github.com/antonillos/asdf-safeselect).

## Release Assets

Each GitHub release contains:

- `safeselect-v<version>-<target>.tar.gz` — compiled binary + embedded sidecar
- `safeselect-v<version>-<target>.tar.gz.sha256` — checksum

## Binary Contents

The `safeselect` binary includes:
- Rust CLI + MCP server
- Embedded `safeselect-sidecar.jar` with the PostgreSQL JDBC bridge, MongoDB driver, and Jackson

**Not included**: JDBC drivers. PostgreSQL users install one separately via
`safeselect driver download --vendor postgresql`; the MongoDB driver is embedded.
