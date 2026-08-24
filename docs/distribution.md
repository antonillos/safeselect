# Distribution

SafeSelect is distributed as a single Rust binary with an embedded Java sidecar
JAR. No installation of the sidecar is needed — it is extracted at runtime.

## Release Process

1. Merge the feature PRs into `develop`.
2. Run **Prepare Release** with `base_branch=develop`; it bumps `Cargo.toml`,
   `Cargo.lock`, and `sidecar/pom.xml`, updates `CHANGELOG.md`, and opens a
   signed release PR back into `develop`.
3. Merge that release PR, then open the release PR from `develop` to `main`.
   This is the only PR that may promote a release to the public branch.
4. Wait for every required Verify check on the `develop` → `main` PR, apply
   `safe-to-merge` only when they are green, and merge through the normal
   reviewed flow. Do not publish to any directory before this merge.
5. The version change on `main` starts the release workflow; it can also be
   dispatched manually with an explicit tag and target ref.
6. Integration tests must pass before the GitHub release and assets are published.
7. GitHub Actions builds for 4 targets:
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
