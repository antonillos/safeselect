# Read-only is not a boolean

> Reviewed: 2026-08-28 · A practical checklist for database access by coding agents.

"Read-only" is a useful label. It is not a complete threat model.

An agent connected to a database needs answers to several different questions:
what can it ask for, what actually executes, what can it read, how much work can
it cause, and what happens when a request crosses the boundary?

## 1. A prompt is a request, not a permission system

Telling an agent "only run SELECT" helps describe the task. It does not remove
write permissions from a shell or a database client. Start by reviewing the
actual tools, credentials and files the agent can reach.

## 2. Tool selection and execution controls are different

A server can hide write tools, validate a query, enforce database transaction
rules, or combine those controls. Do not assume that all products use the same
mechanism because their descriptions contain the same two words.

For example, [MongoDB's local MCP security guidance](https://www.mongodb.com/docs/mcp-server/local-mcp/security-best-practices/)
documents a read-only tool setting alongside a dedicated read-only database
user. [DBHub](https://dbhub.ai/tools/execute-sql#read-only-mode) documents both
classification and engine-level enforcement, with engine and privilege caveats.
Those are concrete contracts to inspect, not evidence that either product is
universally safe or unsafe.

## 3. A read can still be sensitive or expensive

Preventing mutation does not prevent disclosure of readable data. A query that
returns five rows can scan millions before returning them. Set access policy
and execution limits; use sanitized data or a suitable replica where possible.
Do not equate a final SQL LIMIT with a bound on database work.

## 4. Failure behavior belongs in the contract

Should a rejected operation return an error or end the session? What happens
when audit storage fails? Who can change policy? How does an operator recover?
There is an availability tradeoff here, not a one-word security ranking.

SafeSelect terminates its MCP process on security violations. That makes the
boundary explicit, but means the cause must be corrected before restarting.

## 5. Ask for evidence with a scope

SafeSelect's design validates requests in Rust and again in its embedded Java
sidecar. The [Security Proof](security-proof.md) maps claims to code, tests and
limits. The [suite contract](security-test-suite.md) distinguishes defined cases
from executed tests and requires disposable fixtures for real database runs.

Examples include stacked SQL, data-modifying CTEs and MongoDB aggregation write
stages. Reproduce them only in the documented disposable test environment.
Do not submit destructive probes to a real production database to "check" a
marketing claim.

## The useful question

Instead of "does this server have a read-only flag?", ask:

> Which operations and data can this agent reach, under this configuration,
> with these database privileges—and what evidence demonstrates the boundary?

SafeSelect is one answer for local PostgreSQL and MongoDB inspection. It does
not replace least-privilege database roles, protect a compromised host or govern
another tool's connection. Our [comparison](compare.md) explains where other
approaches fit better.

**Try one bounded read:** follow [DBeaver → Codex](guides/dbeaver-codex.md).
