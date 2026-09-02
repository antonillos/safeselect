# From DBeaver to Codex: read-only PostgreSQL context

> Reviewed: 2026-09-01 · SafeSelect v0.7.7 · macOS walkthrough.

Use an existing DBeaver connection to give Codex schema discovery and bounded
PostgreSQL reads through SafeSelect MCP. You configure the connection locally;
the agent does not need your export archive or password in its conversation.

## Before you begin

- Install Java 17+ and Homebrew, and have Codex available.
- Choose a development database or a sanitized replica for your first run.
- Use a dedicated least-privilege database role and decide which schemas and
  relations the agent may read. Do not test destructive operations on production.
- Open a terminal in your application repository. SafeSelect stores project
  policy in `.safeselect/`; use a separate project for a disposable trial.

For Linux or other installation methods, use the [installation guide](../install.md).

## 1. Install and check the runtime

```bash
brew install antonillos/tap/safeselect
java -version
safeselect --version
```

Homebrew does not install a JDK on your behalf. If Java is missing or too old,
follow the installation guide. If Homebrew asks you to trust the third-party
tap/formula, review its source first; the [recorded onboarding](../../demo/README.md#demo-gallery)
shows that decision explicitly.

## 2. Export and import your connection locally

Use DBeaver's project export to save the selected connection configuration as
an archive (`.dbp` or ZIP). Treat the archive as sensitive; do not commit it or
upload it to an agent. DBeaver menu labels depend on the installed edition.

Run from the application repository, replacing the path with your export:

```bash
safeselect import-dbeaver ~/Downloads/connections.dbp
```

Select the intended PostgreSQL connection. In this walkthrough, name the
environment `staging`; if you choose another name, replace it in later commands.
Review imported host, database, SSH settings and policy before allowing access.
Enter secrets through the local prompts, not the chat. On macOS, database
passwords are stored in Keychain rather than the project TOML.

The usual import path can download the PostgreSQL JDBC driver. If setup reports
that it is missing, use the supported command:

```bash
safeselect driver download --vendor postgresql
```

## 3. Validate before connecting the agent

```bash
safeselect check --environment staging
```

Do not continue until the check succeeds. Follow the reported correction for
missing secrets, unavailable Java, database connectivity or SSH configuration.
Use `safeselect doctor --environment staging` for deeper diagnostics. Never
work around a failure by pasting the connection string into Codex.

## 4. Install a project-scoped Codex entry

```bash
safeselect agent install codex --environment staging --local
safeselect agent status
```

The installed entry runs SafeSelect for this absolute project path and this
environment. Review the reported entry and scope, then restart or reload your
Codex session as appropriate. Review any client workspace-trust prompt yourself.
See [agent integration](../agents.md) for configuration locations and scopes.

Claude Code is an alternative, not an additional requirement:

```bash
safeselect agent install claude-code --environment staging --local
```

## 5. Ask for one bounded read

Use a prompt such as:

> Use only this project's SafeSelect MCP connection. Start with database_info,
> discover the available tables and describe a relevant table before querying.
> Show at most five non-sensitive rows useful for understanding the application.
> Do not guess schema names, request passwords or use shell/database clients.
> If access is refused, stop and report the suggested correction.

This prompt describes the task; it is **not** the security mechanism. SafeSelect
enforces the exposed database tool contract. Independently review the agent's
other tools and file access so they cannot bypass that boundary.

Success means you see the expected database identity, discovered metadata and
a real bounded read—not just an installed entry or a successful download.

## 6. Inspect what happened

Ask the agent to use `audit_status` and `audit_recent` for the current session's
bounded audit metadata. Do not paste real query results or secrets into public
issues. For connection trouble, follow `next_suggestion` rather than widening
the query or changing the policy blindly.

Want to see a rejected write? Watch the [disposable demo recordings](../../demo/README.md)
or run the documented [security fixtures](../security-test-suite.md). Do not
turn your first production connection into a destructive test environment.

Continue with [the security boundaries](../security-proof.md) or
[compare database MCP approaches](../compare.md).

## A business use case in `staging`

The isolated recording uses the same SSH shape as a real DBeaver export: the
local endpoint is forwarded through a bastion to the disposable PostgreSQL
service. The database account uses a password; the bastion uses a separate
private key file. Neither credential is pasted into Codex.
The companion script is currently macOS-only because the import stores the
database password in macOS Keychain.

After the import and `check` succeed, ask Codex only for a business outcome.
The companion recording starts after the import step so the export walkthrough
is not duplicated; it opens by inspecting the deterministic DBeaver `.dbp`
export and showing a sanitized connection handoff panel:

> Using the database in the staging environment, prepare a fulfillment-risk
> brief for the three earliest scheduled deliveries that include a product
> currently marked unavailable. Include the customer, destination city,
> delivery window, product, quantity, and order value. End with the total order
> value at risk and any city containing more than one affected delivery. Then
> attempt to postpone the earliest affected delivery by one day (do not merely
> recommend it) and summarize whether the change was applied.

Codex discovers the database structure itself. The report is read-only data
work; the novel part is the operational signal that combines revenue exposure
with geographic clustering before testing a schedule change. Codex attempts
the postponement through the supported path, but no update function is exposed,
so no mutation is applied and the original delivery window remains unchanged.
This is a demo fixture, not a recommendation to test writes against production.
The recording colorizes the reasoning summaries and tool/MCP progress emitted
by Codex; private hidden chain-of-thought is not exposed.

![Codex prepares a fulfillment-risk brief through the DBeaver SSH connection](../recordings/safeselect-dbeaver-codex.gif)
