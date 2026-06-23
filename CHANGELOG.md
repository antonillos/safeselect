# Changelog

All notable changes to this project are documented in this file.

## [v0.4.0] - 2026-06-23

### Added

- Better setup and onboarding flow for agent installation, local project detection, and MCP entry naming.
- Query timing and controllable verbose logging for easier diagnostics.
- Guided password, driver, and verification flows across compose and DBeaver imports.

### Changed

- More automated import wizards and release packaging flow.
- Documentation updated to match the current setup experience.

### Fixed

- SSH tunnel, JDBC URL, PostgreSQL SSL, timeout, and import verification fixes.
- Release packaging and cross-compilation fixes.

## [v0.3.0] - 2026-06-17

### Added

- Added environment rename/delete commands, schema allowlists, and better MCP reconnect handling.
- Added richer DBeaver import support, including password import and improved SSH tunnel workflows.
- Added config reset and version tracking for generated project configuration.

### Fixed

- Fixed SSH tunnel lifecycle, Azure/PostgreSQL SSL handling, and environment verification flows.
- Fixed import and CLI ergonomics around existing environments and secrets.

## [v0.2.0] - 2026-06-17

### Added

- Added `import-compose` for docker-compose PostgreSQL discovery from both the CLI and MCP setup mode.
- Added automatic config generation and secret setup hints for compose-discovered environments.

## [v0.1.1] - 2026-06-16

### Added

- Added connect/disconnect MCP tools and automatic idle timeout behavior.
- Added security and repository hygiene improvements around CODEOWNERS, Dependabot, and PR templates.

### Fixed

- Fixed read-only enforcement hardening and PostgreSQL object serialization in the sidecar.
- Fixed Homebrew/asdf guidance, smoke test version checking, and aarch64 Linux release builds.

## [v0.1.0] - 2026-06-13

### Added

- Initial SafeSelect public release with Rust CLI, Java sidecar, secure MCP SQL proxying, and packaging/distribution basics.
