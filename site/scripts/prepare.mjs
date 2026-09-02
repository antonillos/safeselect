import { readFile, writeFile, mkdir, copyFile } from "node:fs/promises";
import { resolve, dirname, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { marked } from "marked";
import { existsSync } from "node:fs";

const site = resolve(dirname(fileURLToPath(import.meta.url)), "..");
// The public repository keeps editorial Markdown in ../docs. An isolated
// Sites source snapshot includes that exact Markdown under source-docs/docs.
const root = existsSync(resolve(site, "../docs/compare.md"))
  ? resolve(site, "..")
  : resolve(site, "source-docs");
const definitions = [
  {
    file: "docs/compare.md",
    route: "/compare/",
    title: "Read-only database MCP comparison | SafeSelect MCP",
    description:
      "Compare SafeSelect, DBHub, MongoDB MCP Server, Postgres MCP Pro and SchemaBrain: contracts, deployment, evidence, tradeoffs and limits.",
  },
  {
    file: "docs/guides/dbeaver-codex.md",
    route: "/guides/dbeaver-codex/",
    title: "DBeaver to Codex: read-only PostgreSQL MCP | SafeSelect",
    description:
      "Import a DBeaver connection, validate PostgreSQL access and give Codex a project-scoped SafeSelect MCP connection without pasting credentials into chat.",
  },
  {
    file: "docs/read-only-is-not-a-boolean.md",
    route: "/read-only-is-not-a-boolean/",
    title: "Read-only is not a boolean | SafeSelect MCP",
    description:
      "A practical checklist for database MCP security: tool scope, execution controls, data exposure, limits, failure behavior and reproducible evidence.",
  },
];
const routes = new Map(
  definitions.map((doc) => [resolve(root, doc.file), doc.route]),
);
const assetRoutes = new Map([
  [resolve(root, "docs/recordings/safeselect-dbeaver-codex.gif"), "/dbeaver-codex.gif"],
]);
const documents = {};
for (const doc of definitions) {
  const markdown = await readFile(resolve(root, doc.file), "utf8");
  const tokens = marked.lexer(markdown);
  marked.walkTokens(tokens, (token) => {
    if (
      (token.type !== "link" && token.type !== "image") ||
      /^(https?:|mailto:|#)/.test(token.href)
    )
      return;
    const [path, fragment] = token.href.split("#");
    const target = resolve(root, dirname(doc.file), path);
    if (!target.startsWith(`${root}/`))
      throw new Error(`Link outside repository: ${token.href}`);
    const suffix = fragment ? `#${fragment}` : "";
    token.href = routes.has(target)
      ? `@@BASE@@${routes.get(target)}${suffix}`
      : assetRoutes.has(target)
        ? `@@BASE@@${assetRoutes.get(target)}${suffix}`
      : `https://github.com/antonillos/safeselect/blob/develop/${relative(root, target)}${suffix}`;
  });
  const html = marked
    .parser(tokens)
    .replaceAll(
      "<table>",
      '<div class="table-scroll" tabindex="0" role="region" aria-label="Comparison table"><table>',
    )
    .replaceAll("</table>", "</table></div>");
  documents[doc.route] = { ...doc, html };
}
await mkdir(resolve(site, "content"), { recursive: true });
await writeFile(
  resolve(site, "content/documents.json"),
  `${JSON.stringify(documents, null, 2)}\n`,
);
await copyFile(
  resolve(root, "docs/recordings/onboarding-full-local.gif"),
  resolve(site, "public/onboarding.gif"),
);
const dbeaverRecording = resolve(root, "docs/recordings/safeselect-dbeaver-codex.gif");
if (existsSync(dbeaverRecording)) {
  await copyFile(dbeaverRecording, resolve(site, "public/dbeaver-codex.gif"));
}
console.log(
  `Prepared ${definitions.length} reviewed documents and the onboarding recording.`,
);
