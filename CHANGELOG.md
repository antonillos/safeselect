# Changelog

All notable changes to this project are documented in this file.

## [v0.5.2] - 2026-07-03

### Build And CI

- ci: restore content-based main sync workflow
- chore: promote develop to main

## [v0.5.1] - 2026-06-26

### Added

- feat: add local install script (install.sh)
- feat: add uninstall script
- feat: add uninstall CLI command
- feat: add query subcommand, fix MCP inputSchema field
- feat: interactive connection picker for import-dbeaver
- feat: disconnect/connect MCP tools + auto-idle timeout
- feat: import-compose CLI command + MCP tool for docker-compose PostgreSQL discovery
- feat: import-compose — docker-compose PostgreSQL discovery (CLI + MCP tool) (#14)
- feat: support JSONC comments and opencode.jsonc in agent config
- feat: schema allowlist for MCP tools and auto-reconnect on connection loss
- feat: add rename and delete environment CLI commands
- feat: import DBeaver passwords to Keychain and improve compose import
- feat: embed build date stamp in version string
- feat: auto-generate entry name from project dir and environment
- feat: add 300s timeout to installed MCP entries
- feat: add config set-password command
- feat: show driver and password setup hints after import
- feat: show next step after driver download
- feat: auto-setup driver and passwords during import-compose
- feat: verify keychain password exists, auto-run check after import
- feat: polish import wizard with sections, hidden password, and auto-verify
- feat: lazy sidecar start so MCP server responds immediately to initialize
- feat: polish import-dbeaver with import-compose wizard pattern
- feat: extract SSH key file from DBeaver and print tunnel command on failure
- feat: version tracking, config reset, and version-mismatch detection
- feat: auto-establish SSH tunnels with key file during import and check
- feat: interactive SSH config wizard during import
- feat: auto-install sshpass via Homebrew when password auth needs it
- feat: environment rename/delete, schema allowlist, DBeaver passwords (#16)
- feat: expose all CLI commands as MCP tools and enforce statement_timeout_ms through sidecar
- feat: improve SSH troubleshooting with clear timeout messages and tunnel establishment
- feat: add reconnect CLI command and MCP tool to restart sidecar and verify connection
- feat: check now executes SELECT 1 to verify end-to-end connectivity
- feat: add detailed timing logs to diagnose query timeouts
- feat: add elapsed_ms to query responses and verbose logging control
- feat: add diagnostics for MCP connectivity
- feat: format elapsed durations in query output
- feat: improve agent query diagnostics and docs
- feat: improve agent MCP upgrade and naming
- feat: add MongoDB multibackend support and real security coverage (#32)

### Fixed

- fix: driver permission check too strict (world-read allowed)
- fix: install.sh --debug flag and suppress warnings
- fix: OpenCode MCP entry format (use 'mcp' key not 'mcpServers')
- fix: make agent install idempotent (overwrite existing entry)
- fix: cargo test --lib fails on bin-only crates, use cargo test
- fix: uninstall removes from both 'mcp' and 'mcpServers' keys
- fix: echo client protocol version, add tools.list to capabilities, send logs to stderr
- fix: use ~/.config/safeselect instead of ~/Library/Application Support/safeselect on macOS
- fix: serialize PGobject as string in sidecar; harden read-only enforcement; add disconnect/connect CLI commands
- fix: update asdf repo references, Homebrew caveats for new CLI
- fix: use dynamic version check in smoke test
- fix: install aarch64-linux-gnu cross-compiler for release builds
- fix: install aarch64-linux-gnu cross-compiler for release builds
- fix: set linker env vars for aarch64-linux-gnu cross-compilation
- fix: skip tests for cross-compiled targets in release
- fix: correct table column separators in README
- fix: add missing imported_envs variable in dbeaver import path
- fix: handle multiple sidecar JAR versions in install.sh wildcard
- fix: handle multiple sidecar JARs and add build stamp in release CI
- fix: only show password warning for newly created environments in import-compose
- fix: show driver install hint even when all envs already exist
- fix: only suggest check after driver download
- fix: check existing envs for missing password after import
- fix: remove duplicate next-step from driver download, clean import flow
- fix: debug password setup and use stdin read for prompt
- fix: increase MCP timeout from 300ms to 30000ms
- fix: verify all selected environments, not just new ones
- fix: rewrite JDBC URL to use local tunnel endpoint when SSH is enabled
- fix: use ToSocketAddrs instead of SocketAddr parse for SSH tunnel check
- fix: prefer #-prefixed DBeaver SSH keys for bastion host/port
- fix: kill stale SSH tunnels and add sslmode=require for Azure
- fix: kill any process on port, default reset prompts to Yes
- fix: remove BatchMode=yes for password SSH auth
- fix: rewrite env file when SSH configured, detach SSH child process
- fix: correct sshpass argument order
- fix: parse DBeaver userName (camelCase) as database username
- fix: don't kill active SSH tunnels
- fix: remove automatic sslmode=require from JDBC URL
- fix: try esolitos/ipa/sshpass tap first, fallback to hudochenkov
- fix: remove brew tap auto-install, just guide user to install sshpass
- fix: verify PostgreSQL protocol before accepting tunnel as active
- fix: disable PostgreSQL SSL when using SSH tunnel
- fix: always establish own SSH tunnel when possible
- fix: remove sslmode=disable, use driver default (prefer)
- fix: don't rewrite JDBC URL for SSH, don't kill bastion, double check tunnel+postgres
- fix: auto-establish SSH tunnel in cmd_check when database host unreachable
- fix: wait up to 30s for SSH tunnel to establish
- fix: check PostgreSQL via both original target and tunnel endpoint
- fix: JDBC URL uses SSH tunnel endpoint (localhost:bastion_port)
- fix: set local forward port to 15432 and use sslmode=require
- fix: prevent MCP server hang on sidecar timeout with auto-restart and pipe read timeout
- fix: make pipe read timeout respect statement_timeout_ms config
- fix: establish SSH tunnel before reconnect in CLI and MCP
- fix: add timing logs to reconnect for timeout diagnosis
- fix: add fast bastion check before SSH tunnel in reconnect
- fix: detect sidecar timeout and do immediate full restart
- fix: establish SSH tunnel in MCP check when PostgreSQL unreachable
- fix: make MCP client timeout respect project statement_timeout_ms
- fix: reduce pipe read timeout buffer to prevent zombie queries
- fix: improve timeout recovery and prevent zombie queries
- fix: tolerate existing SSH tunnels in check
- fix: recover connections after SSH tunnel resets
- fix: fail fast when SSH tunnel is unavailable
- fix: reuse tunnel checks for MCP reconnect
- fix: preflight SSH before MCP queries
- fix: restart sidecar on MCP connect
- fix: refresh sidecar for SSH MCP queries
- fix: drop affected rows from query results
- fix: harden query safety and sidecar recovery
- fix: block unsafe read-only functions
- fix: fail close MCP after SQL errors
- fix: detect opencode project config at correct paths
- fix: allow read-only explain analyze
- fix: strip SQL comments before read-only validation
- fix: keep MCP server alive after user SQL errors
- fix: fail-closed only on security violations
- fix: handle SqlError in CLI query command and clean unused imports
- fix: allow safe read-only CTE queries
- fix: update quinn-proto to 0.11.15
- fix: add source_branch input to prepare-release workflow
- fix: resolve remaining merge conflicts in src/main.rs
- fix: handle closed PRs in prepare-release workflow

### Documentation

- docs: badges layout - CI/security on second row
- docs: update README to match actual CLI and config, make less dense
- docs: add architecture diagram to README
- docs: move architecture diagram after quick start, center it
- docs: move architecture above quick start, widen to 800px
- docs: restore security, rust, java, brew, asdf badges
- docs: update README, skill manifest, and Homebrew packaging
- docs: update Quick Start and CLI Reference to reflect recent changes
- docs: simplify no-secret error and add advanced config section
- docs: add missing MCP tools to table, fix blank lines
- docs: sync README with latest changes
- docs: sync README with SSH tunnel, config reset, version tracking
- docs: add changelog-driven release notes
- docs: align release docs and changelog
- docs: tighten README and architecture diagram
- docs: align README setup mode wording
- docs: sharpen safeselect positioning

### Build And CI

- chore: remove PLAN.md, PHASES.md, Formula/ (in external tap)
- ci: add workflows from makevn adapted for safeselect
- ci: fix verify.yml — add Java + sidecar build before cargo check
- chore: suppress dead_code warnings globally (v1)
- ci: install cargo-audit and cargo-deny in CI runner
- ci: fix audit/deny install - check if exists first
- ci: split verify into 3 parallel jobs - unit, integration, security
- chore: ignore fxhash unmaintained advisory in cargo-deny
- ci: run security only when PR has safe-to-merge label; use cargo-binstall
- chore: make repo public, add security files (CODEOWNERS, dependabot, PR template, issue config)
- ci: add develop branch triggers
- chore: bump inquire to 0.9.4, remove fxhash deny exception
- chore: improve asdf plugin scripts, add latest-stable
- chore: improve config error messages with platform-specific hints
- ci: add sync-main-to-develop workflow
- chore: sync main into develop (#20)
- chore: sync main into develop (#23)
- ci: add integration tests before release
- chore: ignore project opencode config
- ci: clarify release pr branch ancestry failures

### Other

- Initial commit: SafeSelect 0.1.0
- Update Homebrew formula with correct homepage
- security: password via stdin, SQL injection fix, size limits
- chore(deps): bump actions/checkout from 4 to 6
- chore(deps): bump all Dependabot PRs
- test: smoke tests + Docker-based integration test
- security: add cargo audit + cargo deny to CI, badges in README
- fix(dbeaver): handle JSON map format, SSH tunnel, clean env names
- refactor: per-project .safeselect/ config instead of global projects dir
- release: bump to 0.1.1 (#13)
- release: bump to 0.2.0 (#15)
- debug: add verbose logging to diagnose sidecar auth errors
- Merge develop: SSH tunnel, Azure SSL, config reset (#19)
- release: bump to 0.3.0 (#21)
- develop to main (#22)
- test: add real security regression suite
- test: cover reconnect after postgres restart
- test: add real smoke coverage
- test: separate smoke and security suites
- style: format rust sources and tests
- release: bump to 0.4.0 (#26) (#27)
- chore(deps): bump actions/upload-artifact from 4 to 7 (#28)
- chore(deps): bump actions/cache from 4 to 6 (#29)
- chore(deps): bump actions/checkout from 4 to 7 (#30)
- feat(compose): improve import guidance and env resolution
- test(compose): cover import guidance flows
- chore(git): ignore IntelliJ project files
- feat(dbeaver): clarify incomplete shared SSH tunnel imports
- fix(config): preserve keychain account references
- fix(agent): prefer local config when uninstalling
- fix(compose): use env secrets outside macos
- fix(ssh): assign distinct local ports per environment
- build(sidecar): stop tracking generated reduced pom
- test(ssh): cover legacy local port allocation
- feat(dbeaver): reuse bastion config during import
- feat(ssh): share bastion config across environments
- feat(agent): make install and uninstall more convention based
- feat(config): uninstall project config بالكامل
- chore(deps): bump com.fasterxml.jackson.core:jackson-databind (#31)
- test: expand real security coverage
- release: bump to 0.5.0 (#33)
- promote develop to main (#36)
- promote develop to main (#39)

## [v0.5.0] - 2026-06-26

### Added

- feat: add local install script (install.sh)
- feat: add uninstall script
- feat: add uninstall CLI command
- feat: add query subcommand, fix MCP inputSchema field
- feat: interactive connection picker for import-dbeaver
- feat: disconnect/connect MCP tools + auto-idle timeout
- feat: import-compose CLI command + MCP tool for docker-compose PostgreSQL discovery
- feat: import-compose — docker-compose PostgreSQL discovery (CLI + MCP tool) (#14)
- feat: support JSONC comments and opencode.jsonc in agent config
- feat: schema allowlist for MCP tools and auto-reconnect on connection loss
- feat: add rename and delete environment CLI commands
- feat: import DBeaver passwords to Keychain and improve compose import
- feat: embed build date stamp in version string
- feat: auto-generate entry name from project dir and environment
- feat: add 300s timeout to installed MCP entries
- feat: add config set-password command
- feat: show driver and password setup hints after import
- feat: show next step after driver download
- feat: auto-setup driver and passwords during import-compose
- feat: verify keychain password exists, auto-run check after import
- feat: polish import wizard with sections, hidden password, and auto-verify
- feat: lazy sidecar start so MCP server responds immediately to initialize
- feat: polish import-dbeaver with import-compose wizard pattern
- feat: extract SSH key file from DBeaver and print tunnel command on failure
- feat: version tracking, config reset, and version-mismatch detection
- feat: auto-establish SSH tunnels with key file during import and check
- feat: interactive SSH config wizard during import
- feat: auto-install sshpass via Homebrew when password auth needs it
- feat: environment rename/delete, schema allowlist, DBeaver passwords (#16)
- feat: expose all CLI commands as MCP tools and enforce statement_timeout_ms through sidecar
- feat: improve SSH troubleshooting with clear timeout messages and tunnel establishment
- feat: add reconnect CLI command and MCP tool to restart sidecar and verify connection
- feat: check now executes SELECT 1 to verify end-to-end connectivity
- feat: add detailed timing logs to diagnose query timeouts
- feat: add elapsed_ms to query responses and verbose logging control
- feat: add diagnostics for MCP connectivity
- feat: format elapsed durations in query output
- feat: improve agent query diagnostics and docs
- feat: improve agent MCP upgrade and naming
- feat: add MongoDB multibackend support and real security coverage (#32)

### Fixed

- fix: driver permission check too strict (world-read allowed)
- fix: install.sh --debug flag and suppress warnings
- fix: OpenCode MCP entry format (use 'mcp' key not 'mcpServers')
- fix: make agent install idempotent (overwrite existing entry)
- fix: cargo test --lib fails on bin-only crates, use cargo test
- fix: uninstall removes from both 'mcp' and 'mcpServers' keys
- fix: echo client protocol version, add tools.list to capabilities, send logs to stderr
- fix: use ~/.config/safeselect instead of ~/Library/Application Support/safeselect on macOS
- fix: serialize PGobject as string in sidecar; harden read-only enforcement; add disconnect/connect CLI commands
- fix: update asdf repo references, Homebrew caveats for new CLI
- fix: use dynamic version check in smoke test
- fix: install aarch64-linux-gnu cross-compiler for release builds
- fix: install aarch64-linux-gnu cross-compiler for release builds
- fix: set linker env vars for aarch64-linux-gnu cross-compilation
- fix: skip tests for cross-compiled targets in release
- fix: correct table column separators in README
- fix: add missing imported_envs variable in dbeaver import path
- fix: handle multiple sidecar JAR versions in install.sh wildcard
- fix: handle multiple sidecar JARs and add build stamp in release CI
- fix: only show password warning for newly created environments in import-compose
- fix: show driver install hint even when all envs already exist
- fix: only suggest check after driver download
- fix: check existing envs for missing password after import
- fix: remove duplicate next-step from driver download, clean import flow
- fix: debug password setup and use stdin read for prompt
- fix: increase MCP timeout from 300ms to 30000ms
- fix: verify all selected environments, not just new ones
- fix: rewrite JDBC URL to use local tunnel endpoint when SSH is enabled
- fix: use ToSocketAddrs instead of SocketAddr parse for SSH tunnel check
- fix: prefer #-prefixed DBeaver SSH keys for bastion host/port
- fix: kill stale SSH tunnels and add sslmode=require for Azure
- fix: kill any process on port, default reset prompts to Yes
- fix: remove BatchMode=yes for password SSH auth
- fix: rewrite env file when SSH configured, detach SSH child process
- fix: correct sshpass argument order
- fix: parse DBeaver userName (camelCase) as database username
- fix: don't kill active SSH tunnels
- fix: remove automatic sslmode=require from JDBC URL
- fix: try esolitos/ipa/sshpass tap first, fallback to hudochenkov
- fix: remove brew tap auto-install, just guide user to install sshpass
- fix: verify PostgreSQL protocol before accepting tunnel as active
- fix: disable PostgreSQL SSL when using SSH tunnel
- fix: always establish own SSH tunnel when possible
- fix: remove sslmode=disable, use driver default (prefer)
- fix: don't rewrite JDBC URL for SSH, don't kill bastion, double check tunnel+postgres
- fix: auto-establish SSH tunnel in cmd_check when database host unreachable
- fix: wait up to 30s for SSH tunnel to establish
- fix: check PostgreSQL via both original target and tunnel endpoint
- fix: JDBC URL uses SSH tunnel endpoint (localhost:bastion_port)
- fix: set local forward port to 15432 and use sslmode=require
- fix: prevent MCP server hang on sidecar timeout with auto-restart and pipe read timeout
- fix: make pipe read timeout respect statement_timeout_ms config
- fix: establish SSH tunnel before reconnect in CLI and MCP
- fix: add timing logs to reconnect for timeout diagnosis
- fix: add fast bastion check before SSH tunnel in reconnect
- fix: detect sidecar timeout and do immediate full restart
- fix: establish SSH tunnel in MCP check when PostgreSQL unreachable
- fix: make MCP client timeout respect project statement_timeout_ms
- fix: reduce pipe read timeout buffer to prevent zombie queries
- fix: improve timeout recovery and prevent zombie queries
- fix: tolerate existing SSH tunnels in check
- fix: recover connections after SSH tunnel resets
- fix: fail fast when SSH tunnel is unavailable
- fix: reuse tunnel checks for MCP reconnect
- fix: preflight SSH before MCP queries
- fix: restart sidecar on MCP connect
- fix: refresh sidecar for SSH MCP queries
- fix: drop affected rows from query results
- fix: harden query safety and sidecar recovery
- fix: block unsafe read-only functions
- fix: fail close MCP after SQL errors
- fix: detect opencode project config at correct paths
- fix: allow read-only explain analyze
- fix: strip SQL comments before read-only validation
- fix: keep MCP server alive after user SQL errors
- fix: fail-closed only on security violations
- fix: handle SqlError in CLI query command and clean unused imports
- fix: allow safe read-only CTE queries
- fix: update quinn-proto to 0.11.15
- fix: add source_branch input to prepare-release workflow
- fix: resolve remaining merge conflicts in src/main.rs
- fix: handle closed PRs in prepare-release workflow

### Documentation

- docs: badges layout - CI/security on second row
- docs: update README to match actual CLI and config, make less dense
- docs: add architecture diagram to README
- docs: move architecture diagram after quick start, center it
- docs: move architecture above quick start, widen to 800px
- docs: restore security, rust, java, brew, asdf badges
- docs: update README, skill manifest, and Homebrew packaging
- docs: update Quick Start and CLI Reference to reflect recent changes
- docs: simplify no-secret error and add advanced config section
- docs: add missing MCP tools to table, fix blank lines
- docs: sync README with latest changes
- docs: sync README with SSH tunnel, config reset, version tracking
- docs: add changelog-driven release notes
- docs: align release docs and changelog
- docs: tighten README and architecture diagram
- docs: align README setup mode wording
- docs: sharpen safeselect positioning

### Build And CI

- chore: remove PLAN.md, PHASES.md, Formula/ (in external tap)
- ci: add workflows from makevn adapted for safeselect
- ci: fix verify.yml — add Java + sidecar build before cargo check
- chore: suppress dead_code warnings globally (v1)
- ci: install cargo-audit and cargo-deny in CI runner
- ci: fix audit/deny install - check if exists first
- ci: split verify into 3 parallel jobs - unit, integration, security
- chore: ignore fxhash unmaintained advisory in cargo-deny
- ci: run security only when PR has safe-to-merge label; use cargo-binstall
- chore: make repo public, add security files (CODEOWNERS, dependabot, PR template, issue config)
- ci: add develop branch triggers
- chore: bump inquire to 0.9.4, remove fxhash deny exception
- chore: improve asdf plugin scripts, add latest-stable
- chore: improve config error messages with platform-specific hints
- ci: add sync-main-to-develop workflow
- chore: sync main into develop (#20)
- chore: sync main into develop (#23)
- ci: add integration tests before release
- chore: ignore project opencode config
- ci: clarify release pr branch ancestry failures

### Other

- Initial commit: SafeSelect 0.1.0
- Update Homebrew formula with correct homepage
- security: password via stdin, SQL injection fix, size limits
- chore(deps): bump actions/checkout from 4 to 6
- chore(deps): bump all Dependabot PRs
- test: smoke tests + Docker-based integration test
- security: add cargo audit + cargo deny to CI, badges in README
- fix(dbeaver): handle JSON map format, SSH tunnel, clean env names
- refactor: per-project .safeselect/ config instead of global projects dir
- release: bump to 0.1.1 (#13)
- release: bump to 0.2.0 (#15)
- debug: add verbose logging to diagnose sidecar auth errors
- Merge develop: SSH tunnel, Azure SSL, config reset (#19)
- release: bump to 0.3.0 (#21)
- develop to main (#22)
- test: add real security regression suite
- test: cover reconnect after postgres restart
- test: add real smoke coverage
- test: separate smoke and security suites
- style: format rust sources and tests
- release: bump to 0.4.0 (#26) (#27)
- chore(deps): bump actions/upload-artifact from 4 to 7 (#28)
- chore(deps): bump actions/cache from 4 to 6 (#29)
- chore(deps): bump actions/checkout from 4 to 7 (#30)
- feat(compose): improve import guidance and env resolution
- test(compose): cover import guidance flows
- chore(git): ignore IntelliJ project files
- feat(dbeaver): clarify incomplete shared SSH tunnel imports
- fix(config): preserve keychain account references
- fix(agent): prefer local config when uninstalling
- fix(compose): use env secrets outside macos
- fix(ssh): assign distinct local ports per environment
- build(sidecar): stop tracking generated reduced pom
- test(ssh): cover legacy local port allocation
- feat(dbeaver): reuse bastion config during import
- feat(ssh): share bastion config across environments
- feat(agent): make install and uninstall more convention based
- feat(config): uninstall project config بالكامل
- chore(deps): bump com.fasterxml.jackson.core:jackson-databind (#31)
- test: expand real security coverage

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
