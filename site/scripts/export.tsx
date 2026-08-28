import { mkdir, readFile, writeFile, copyFile } from "node:fs/promises";
import { renderToStaticMarkup } from "react-dom/server";
import Home from "../app/page";
import { Document, type DocumentRoute } from "../app/document";
import documents from "../content/documents.json";
import { CANONICAL } from "../app/shared";
import { homeTitle, homeDescription } from "../app/metadata";

// Static HTML shares the exact components/content with the Sites preview.
// GitHub Pages needs no Node runtime, hydration, trackers or external fonts.
process.env.NEXT_PUBLIC_SITE_BASE_PATH = "/safeselect";
const pages = [
  {
    route: "/",
    title: homeTitle,
    description: homeDescription,
    content: <Home />,
  },
  ...Object.entries(documents).map(([route, doc]) => ({
    ...doc,
    route,
    content: <Document route={route as DocumentRoute} />,
  })),
];
await mkdir("out", { recursive: true });
for (const page of pages) {
  const head = renderToStaticMarkup(
    <head>
      <meta charSet="utf-8" />
      <meta name="viewport" content="width=device-width, initial-scale=1" />
      <title>{page.title}</title>
      <meta name="description" content={page.description} />
      <link rel="icon" href="/safeselect/icon.svg" type="image/svg+xml" />
      <link rel="icon" href="/safeselect/favicon-32.png" type="image/png" sizes="32x32" />
      <link rel="apple-touch-icon" href="/safeselect/apple-touch-icon.png" sizes="180x180" />
      <link rel="canonical" href={`${CANONICAL}${page.route}`} />
      <meta property="og:site_name" content="SafeSelect MCP" />
      <meta property="og:title" content={page.title} />
      <meta property="og:description" content={page.description} />
      <meta property="og:url" content={`${CANONICAL}${page.route}`} />
      <meta
        property="og:type"
        content={page.route === "/" ? "website" : "article"}
      />
      <meta
        name="twitter:card"
        content={page.route === "/" ? "summary_large_image" : "summary"}
      />
      <meta name="twitter:title" content={page.title} />
      <meta name="twitter:description" content={page.description} />
      {page.route === "/" && (
        <>
          <meta property="og:image" content={`${CANONICAL}/og.png`} />
          <meta
            property="og:image:alt"
            content="SafeSelect MCP: read-only PostgreSQL and MongoDB for coding agents"
          />
          <meta name="twitter:image" content={`${CANONICAL}/og.png`} />
        </>
      )}
      <link rel="stylesheet" href="/safeselect/site.css" />
    </head>,
  );
  const directory = `out${page.route}`;
  await mkdir(directory, { recursive: true });
  await writeFile(
    `${directory}index.html`,
    `<!doctype html><html lang="en">${head}<body>${renderToStaticMarkup(page.content)}</body></html>\n`,
  );
}
const css = (await readFile("app/globals.css", "utf8")).replace(
  /^@import ['"]tailwindcss['"];?\s*/m,
  "",
);
await writeFile("out/site.css", css);
await copyFile("public/og.png", "out/og.png");
for (const asset of ["icon.svg", "icon-512.png", "favicon-32.png", "apple-touch-icon.png"]) {
  await copyFile(`public/${asset}`, `out/${asset}`);
}
await copyFile("public/onboarding.gif", "out/onboarding.gif");
// Keep Google's supplied ownership proof byte-for-byte on every Pages deploy.
await copyFile(
  "public/googled7be89f4207cbfe7.html",
  "out/googled7be89f4207cbfe7.html",
);
await writeFile("out/.nojekyll", "");
await writeFile(
  "out/sitemap.xml",
  `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">${pages.map((p) => `<url><loc>${CANONICAL}${p.route}</loc></url>`).join("")}</urlset>\n`,
);
console.log(`Exported ${pages.length} static pages to out/.`);
