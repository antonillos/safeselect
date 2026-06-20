# Security Policy

## Reporting a Vulnerability

SafeSelect is designed to be a security-first tool. If you discover a
security vulnerability, please report it privately to the repository owner.

**Do not** open a public GitHub issue.

Send details to the repository owner via email or a private GitHub contact.
Include:

- Version or commit where the vulnerability exists
- Steps to reproduce
- Expected vs actual behavior
- Potential impact
- Any suggested mitigation or fix (optional)

## Scope

Security fixes are applied to the latest release and backported on a
best-effort basis.

## Supported Versions

| Version | Supported |
|---|---|
| >= 0.1.0 | ✅ |

## Security Features

SafeSelect implements multiple layers of security (all enforced server-side, not configurable by the agent):

- **Fail-closed**: any security incident terminates the MCP process immediately
- **Read-only always enforced**: only SELECT, EXPLAIN, and WITH queries allowed, never configurable
- **Single statement**: multi-statement SQL is rejected unless explicitly allowed
- **SQL validation**: parsed and validated against policy before execution
- **Allowed schemas / denied relations**: policy-based access control per project
- **SHA-256 driver validation**: JDBC drivers are checksummed before each use
- **macOS Keychain**: secrets stored securely, never in config files
- **Password isolation**: passwords passed via stdin, never as CLI args
- **Audit log**: all queries hashed (SHA-256), never stored in plain text
- **Result limits**: row count and byte size limits enforced
- **Auto-disconnect**: configurable idle timeout closes connection after inactivity
- **Explicit query-plan analysis**: `EXPLAIN ANALYZE` is opt-in and still constrained by read-only validation
