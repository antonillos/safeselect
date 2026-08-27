# Read-only database MCP servers: choosing the right boundary

> Reviewed: 2026-08-28. Written by the SafeSelect maintainers. This is a
> documentation-based comparison, not an independent security audit or a
> benchmark. SafeSelect's current release is v0.7.6; competitor documentation
> is a dated snapshot of the linked pages, not a guarantee about every version.

An MCP server can make a database easier to use without making every operation
appropriate for an agent. Start with what the agent needs to do, where policy
is enforced, and which credentials and other tools it can access.

## At a glance

| Project | Documented contract and boundary | Deployment and scope | Choose it when |
| --- | --- | --- | --- |
| **SafeSelect MCP** | Fixed database read tools, Rust validation and a second sidecar check; project policy and row/byte/time limits. Security violations terminate the process. | Local MCP stdio; PostgreSQL and MongoDB. Java 17+ required. | Coding agents need direct but constrained database inspection across these two backends, with connection import and inspectable security evidence. |
| **DBHub** | Configurable tools; `readonly = true` combines a keyword classifier with engine read-only enforcement where documented. Limits and custom SQL tools are configurable. | Multiple SQL engines, including PostgreSQL, MySQL, MariaDB, SQL Server and SQLite; stdio/HTTP options. | Multiple SQL engines, a small default tool surface, custom operations or a workbench matter more than a fixed read-only product contract. [Sources: introduction and SQL tool](https://dbhub.ai/tools/execute-sql). |
| **MongoDB MCP Server** | `--readOnly` restricts tools to read, connect and metadata operations. The documented default permits writes. MongoDB recommends a dedicated read-only database user too. | Official MongoDB integration. This row concerns its local server, not every hosted or Atlas configuration. | An official MongoDB-specific integration is the priority and you can manage the read-only setting and database permissions. [Source: security guidance](https://www.mongodb.com/docs/mcp-server/local-mcp/security-best-practices/). |
| **Postgres MCP Pro** | Configurable access: restricted mode uses read-only transactions and an execution-time constraint; unrestricted mode supports writes. | PostgreSQL; local stdio or shared SSE deployment. Performance-analysis tooling is a major part of the offering. | PostgreSQL health analysis, index tuning and DBA workflows are central. [Source: access modes and tools](https://github.com/crystaldba/postgres-mcp#access-mode). |
| **SchemaBrain** | Compiles queries from controlled definitions rather than accepting raw agent SQL; documents PII-aware refusal and tamper-evident auditing. | PostgreSQL source support in its documented beta; a semantic layer rather than a general raw-SQL tool. | Governed entities, joins and metrics are more useful than free-form database inspection. [Source: architecture and current support](https://github.com/Arun-kc/schemabrain). |

DBHub's [introduction](https://dbhub.ai/) describes its supported engines and
deployment options. Its [read-only documentation](https://dbhub.ai/tools/execute-sql#read-only-mode)
explicitly discusses engine-specific limitations and privileged functions.
It should not be described as merely a naive SELECT-prefix wrapper.

## What SafeSelect does—and does not—claim

SafeSelect exposes no database mutation, migration or administration tools.
It applies policy to SQL and MongoDB operations before execution, rather than
asking the agent to remember a rule. The same product provides DBeaver,
Compose and Compass imports, agent installation, scoped discovery, query
limits and audit metadata. See the [README](../README.md),
[security model](security.md) and [Security Proof](security-proof.md).

The boundary applies **only to requests through SafeSelect**. A shell, another
MCP server, exposed credentials or a compromised host can provide a separate
route to the database. Readable data can still be sensitive. Read-only queries
can still cost resources. Native permissions and deliberate data selection
remain necessary.

Fail-closed is a tradeoff: terminating on a security violation reduces continued
use of that session, but the operator must correct the cause before restarting.
It is not evidence that a competitor returning an error is inherently unsafe.

## Evidence, not checkmark theatre

SafeSelect publishes a [threat model and claim-to-test mapping](security-proof.md)
and a [security suite execution contract](security-test-suite.md). Real-database
tests must use explicitly disposable fixtures. A test definition is not a test
run, and a passing suite is not proof against every future bypass.

The competitor rows describe their authors' documentation. We did not execute
their test suites or establish that a feature absent from these pages is absent
from the product. This comparison makes no universal "most secure" ranking.

For your deployment, verify:

1. Which tools are exposed in the actual installed version and configuration?
2. Can a configuration change enable writes? Who controls that configuration?
3. Is enforcement in tool selection, query validation, the driver/database—or
   several layers? What happens if one layer fails?
4. Which data, functions and resources can the effective database role access?
5. Are secrets, query logs and results handled according to your requirements?
6. Can the agent bypass this connection through a different tool?

## SafeSelect versus a read-only database role

These are complementary controls, not mutually exclusive products. A database
role is the native authority on permissions. SafeSelect adds an agent-facing
tool contract, project policy, bounded results, connection setup and auditing.
Use both; do not broaden a production account just to demonstrate the proxy.

## SafeSelect versus handing the agent a shell

A shell with database credentials provides a broader execution surface than a
fixed MCP tool set. SafeSelect can constrain its own connection, but cannot
remove permissions from a shell you also give the agent. Keep configuration
and secrets under operator control, and review the agent's entire tool set.

## Start with your use case

- For **PostgreSQL + MongoDB inspection by coding agents**, try SafeSelect's
  [DBeaver-to-Codex guide](guides/dbeaver-codex.md).
- For **broader SQL connectivity**, evaluate DBHub's documented engines and
  deployment model.
- For **MongoDB platform integration**, evaluate the official server.
- For **PostgreSQL performance workflows**, evaluate Postgres MCP Pro.
- For **curated semantic analytics and PII policy**, evaluate SchemaBrain.

Found an outdated statement? [Open an issue](https://github.com/antonillos/safeselect/issues)
with the relevant version and a primary source. Security reports belong in the
[private disclosure process](../SECURITY.md).
