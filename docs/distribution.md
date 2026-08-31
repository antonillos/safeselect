# Distribution

SafeSelect is distributed as a single Rust binary with an embedded Java sidecar
JAR. No installation of the sidecar is needed — it is extracted at runtime.

## Release Process

1. Merge the feature PRs into `develop` and wait for its required checks.
2. Open the promotion PR from `develop` to `main`, wait for every required
   Verify check, apply `safe-to-merge` only when they are green, and merge
   through the normal reviewed flow.
3. Run **Prepare Release** with `base_branch=main`; it bumps `Cargo.toml`,
   `Cargo.lock`, and `sidecar/pom.xml`, updates `CHANGELOG.md`, and opens a
   signed release PR from `release/vX.Y.Z` into `main`.
4. Merge that release PR after its required checks pass. This is the only PR
   that may change the public release version.
5. The version change on `main` starts the release workflow; it can also be
   dispatched manually with an explicit tag and target ref.
6. After publication, synchronize `main` back into `develop` so both branches
   contain the release version.
7. Integration tests must pass before the GitHub release and assets are published.
8. GitHub Actions builds for 4 targets:
   - `aarch64-apple-darwin`
   - `x86_64-apple-darwin`
   - `aarch64-unknown-linux-gnu`
   - `x86_64-unknown-linux-gnu`
8. Each release includes the binary plus a SHA-256 checksum.
9. Non-draft, non-prerelease releases update Homebrew and asdf when the release token is configured.
10. Only after all MCPB artifacts are attached does the release workflow publish
    their checksummed `server.json` through GitHub OIDC to the official MCP Registry.

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
- `safeselect-v<version>-<target>.mcpb` — platform-specific MCP Bundle
- `safeselect-v<version>-<target>.mcpb.sha256` — MCP Bundle checksum
- `server.json` — checksummed official MCP Registry metadata for that release

The MCP Bundle asks the client for the SafeSelect project directory and
environment at install time. It never embeds database credentials, DSNs, or a
default project, preserving SafeSelect's project-scoped security boundary.

## Binary Contents

The `safeselect` binary includes:
- Rust CLI + MCP server
- Embedded `safeselect-sidecar.jar` with the PostgreSQL JDBC bridge, MongoDB driver, and Jackson

**Not included**: JDBC drivers. PostgreSQL users install one separately via
`safeselect driver download --vendor postgresql`; the MongoDB driver is embedded.
