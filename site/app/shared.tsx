import type { ReactNode } from "react";

export const REPO = "https://github.com/antonillos/safeselect";
export const CANONICAL = "https://antonillos.github.io/safeselect";
export const sitePath = (path: string) =>
  `${process.env.NEXT_PUBLIC_SITE_BASE_PATH ?? ""}${path}`;

export function Shell({ children }: { children: ReactNode }) {
  return (
    <>
      <a className="skip" href="#main">
        Skip to content
      </a>
      <header className="header">
        <a className="brand" href={sitePath("/")}>
          <img className="brand-mark" src={sitePath("/icon.svg")} width="36" height="36" alt="SafeSelect" />
          SafeSelect <span className="brand-mcp">MCP</span>
        </a>
        <nav aria-label="Main navigation">
          <a href={`${REPO}/blob/develop/docs/security-proof.md`}>Security</a>
          <a href={sitePath("/compare/")}>Compare</a>
          <a href={REPO}>GitHub ↗</a>
        </nav>
      </header>
      <main id="main">{children}</main>
      <footer>
        <a className="brand" href={sitePath("/")}>
          SafeSelect MCP
        </a>
        <p>Database visibility. Deliberately constrained.</p>
        <a href={`${REPO}/blob/develop/LICENSE`}>MIT license</a>
        <a href={`${REPO}/blob/develop/SECURITY.md`}>Report a vulnerability</a>
      </footer>
    </>
  );
}
