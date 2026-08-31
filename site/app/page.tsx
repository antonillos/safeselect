import { Shell, sitePath, REPO } from "./shared";

export default function Home() {
  return (
    <Shell>
      <section className="hero">
        <p className="eyebrow">Database context. Not database control.</p>
        <h1>
          Agents can look.
          <br />
          <span>They cannot mutate.</span>
        </h1>
        <p className="intro">
          Read-only PostgreSQL &amp; MongoDB access for coding agents.
        </p>
        <p className="lede">
          Debug with real database context, without exposing write tools.
          SafeSelect MCP puts local, project-scoped policy between your agent
          and your data.
        </p>
        <div className="actions">
          <a className="button" href="#start">
            Start with SafeSelect ↗
          </a>
          <a
            className="text-link"
            href={`${REPO}/blob/develop/docs/security-proof.md`}
          >
            Inspect the security proof →
          </a>
        </div>
        <p className="fine">
          Open source · Local MCP stdio · macOS &amp; Linux · Java 17+
        </p>
        <div
          className="boundary"
          aria-label="Agent requests pass through SafeSelect policy before reaching PostgreSQL or MongoDB"
        >
          <div>
            <small>01 / REQUEST</small>
            <strong>Your coding agent</strong>
            <span>Codex · Claude Code · Cursor · OpenCode</span>
          </div>
          <div className="policy">
            <small>02 / ENFORCE</small>
            <strong>SafeSelect MCP</strong>
            <span>Policy · bounded reads · audit</span>
          </div>
          <div>
            <small>03 / INSPECT</small>
            <strong>Your database</strong>
            <span>PostgreSQL · MongoDB</span>
          </div>
        </div>
        <p className="boundary-note">
          The boundary covers requests through SafeSelect. Use least-privilege
          database roles and keep direct credentials and alternative write tools
          away from the agent.
        </p>
      </section>
      <section id="start" className="section split">
        <div>
          <p className="eyebrow">FROM CONNECTION TO CONTEXT</p>
          <h2>
            Bring the connection
            <br />
            you already use.
          </h2>
          <p>
            Import from DBeaver, Docker Compose or MongoDB Compass. Check the
            environment, then install a project-scoped MCP entry.
          </p>
          <a className="text-link" href={sitePath("/guides/dbeaver-codex/")}>
            Follow DBeaver → Codex →
          </a>
          <p className="fine">
            Run from your application repository. Start with a development
            database or sanitized replica.{" "}
            <a href={`${REPO}/blob/develop/docs/install.md`}>
              Other installation options
            </a>
            .
          </p>
        </div>
        <div className="terminal">
          <div className="terminal-bar">macOS / Homebrew + Java 17+</div>
          <pre>
            <code>{`brew install antonillos/tap/safeselect
safeselect import-dbeaver ~/Downloads/connections.dbp
# Choose staging during import, or use your environment name.
safeselect check --environment staging
safeselect agent install codex --environment staging --local
safeselect agent status`}</code>
          </pre>
          <p>
            Review imports and policy yourself. Never paste an export or
            database password into the agent chat.
          </p>
        </div>
      </section>
      <section className="section">
        <p className="eyebrow">NARROW BY DESIGN</p>
        <h2>Visibility without a write surface.</h2>
        <div className="cards">
          <article>
            <span className="number">01</span>
            <h3>Inspect, don’t administer.</h3>
            <p>
              Discover schemas and collections, inspect bounded results and
              explain queries. No database write or migration tools.
            </p>
          </article>
          <article>
            <span className="number">02</span>
            <h3>Policy stays local.</h3>
            <p>
              Use MCP over stdio, with no MCP network listener. Scope access by
              project and environment, with row, byte and time limits.
            </p>
          </article>
          <article>
            <span className="number">03</span>
            <h3>Check the evidence.</h3>
            <p>
              Security violations terminate the process. Read the threat model
              and reproducible tests—including what they do not guarantee.
            </p>
          </article>
        </div>
      </section>
      <section className="section">
        <p className="eyebrow">THE WHOLE ONBOARDING, NOT A MOCKUP</p>
        <h2>See the first connection.</h2>
        <p>
          The recorded walkthrough uses Homebrew, a DBeaver SSH connection,
          macOS Keychain and OpenCode. It shows a successful read and a rejected
          write against disposable demo data.
        </p>
        <details className="demo">
          <summary>
            Watch the complete onboarding · animated recording, 6.8 MB
          </summary>
          <img
            src={sitePath("/onboarding.gif")}
            width="1280"
            height="720"
            loading="lazy"
            alt="Recorded terminal walkthrough: installing SafeSelect, importing a DBeaver SSH connection, configuring OpenCode, reading an order and rejecting DELETE"
          />
          <p>
            Prefer text? Use the{" "}
            <a href={sitePath("/guides/dbeaver-codex/")}>
              step-by-step Codex guide
            </a>
            . The recording uses OpenCode; the guide explains Codex setup
            separately.
          </p>
        </details>
      </section>
      <section className="section">
        <p className="eyebrow">CHOOSE THE RIGHT TOOL</p>
        <h2>Read-only is a starting point.</h2>
        <div className="cards">
          <article>
            <h3>A comparison, not a ranking.</h3>
            <p>
              DBHub, MongoDB MCP, Postgres MCP Pro and SchemaBrain solve
              different problems. Compare their documented contracts and
              tradeoffs.
            </p>
            <a className="text-link" href={sitePath("/compare/")}>
              Compare approaches →
            </a>
          </article>
          <article>
            <h3>Read-only is not a boolean.</h3>
            <p>
              Tools, execution controls, sensitive reads, resource limits and
              failure behavior are separate questions.
            </p>
            <a
              className="text-link"
              href={sitePath("/read-only-is-not-a-boolean/")}
            >
              Read the checklist →
            </a>
          </article>
          <article>
            <h3>Evidence you can reproduce.</h3>
            <p>
              Inspect the adversarial suite and its disposable-fixture contract.
              A green badge is not a universal security guarantee.
            </p>
            <a
              className="text-link"
              href={`${REPO}/blob/develop/docs/security-test-suite.md`}
            >
              Explore the tests →
            </a>
          </article>
        </div>
      </section>
      <section className="section split">
        <div>
          <p className="eyebrow">DELIBERATE LIMITS</p>
          <h2>Know where it stops.</h2>
        </div>
        <div>
          <h3>Does it replace database permissions?</h3>
          <p>
            No. Use least-privilege roles. SafeSelect constrains its own tool
            surface, not other connections, shell access or a compromised host.
          </p>
          <h3>Does read-only mean no data exposure?</h3>
          <p>
            No. The agent can see authorized results. Choose permitted schemas
            and collections carefully; use sanitized data when appropriate.
          </p>
          <h3>Is this a remote database gateway?</h3>
          <p>
            No. MCP runs locally over stdio. PostgreSQL and MongoDB are the
            supported backends. The embedded Java sidecar requires Java 17+.
          </p>
        </div>
      </section>
      <section className="section endcap">
        <p className="eyebrow">BUILD TRUST WITH EVIDENCE</p>
        <h2>A boundary you can inspect.</h2>
        <p>
          Start with the documented guarantees, limits and disposable security
          fixtures—not a claim that any database connection is risk-free.
        </p>
        <a
          className="button"
          href={`${REPO}/blob/develop/docs/security-proof.md`}
        >
          Read the Security Proof →
        </a>
        <a className="text-link" href={sitePath("/compare/")}>
          Compare approaches →
        </a>
      </section>
    </Shell>
  );
}
