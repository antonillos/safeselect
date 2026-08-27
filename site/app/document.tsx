import type { Metadata } from "next";
import documents from "../content/documents.json";
import { Shell, CANONICAL, sitePath } from "./shared";

export type DocumentRoute = keyof typeof documents;
export function documentMetadata(route: DocumentRoute): Metadata {
  const doc = documents[route];
  return {
    title: doc.title,
    description: doc.description,
    alternates: { canonical: `${CANONICAL}${route}` },
    openGraph: {
      type: "article",
      title: doc.title,
      description: doc.description,
      url: `${CANONICAL}${route}`,
      images: [],
    },
    twitter: {
      card: "summary",
      title: doc.title,
      description: doc.description,
      images: [],
    },
  };
}
export function Document({ route }: { route: DocumentRoute }) {
  // Only build-time Markdown from the three reviewed repository documents.
  // Never pass visitor input or externally fetched HTML into this component.
  const html = documents[route].html.replaceAll("@@BASE@@", sitePath(""));
  return (
    <Shell>
      <article className="article">
        <a className="text-link" href={sitePath("/")}>
          ← SafeSelect MCP
        </a>
        <div dangerouslySetInnerHTML={{ __html: html }} />
      </article>
    </Shell>
  );
}
