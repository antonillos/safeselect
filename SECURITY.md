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

SafeSelect implements multiple layers of security:

- **Fail-closed**: any security incident terminates the MCP process
- **Read-only enforcement**: only SELECT and EXPLAIN queries allowed
- **Single statement**: multi-statement SQL is rejected
- **AST validation**: SQL is parsed and validated before execution
- **Allowed schemas / denied relations**: policy-based access control
- **SHA-256 driver validation**: JDBC drivers are checksummed before each use
- **macOS Keychain**: secrets stored securely, never in config files
- **Password isolation**: passwords passed via stdin, never as CLI args
- **Audit log**: all queries hashed (SHA-256), never stored in plain text
- **Result limits**: row count and byte size limits enforced
