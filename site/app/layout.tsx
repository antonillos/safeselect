import type { Metadata } from "next";
import { CANONICAL } from "./shared";
import { homeTitle, homeDescription } from "./metadata";
import "./globals.css";

export const metadata: Metadata = {
  metadataBase: new URL(`${CANONICAL}/`),
  title: homeTitle,
  description: homeDescription,
  alternates: { canonical: `${CANONICAL}/` },
  openGraph: {
    type: "website",
    siteName: "SafeSelect MCP",
    title: homeTitle,
    description: homeDescription,
    url: `${CANONICAL}/`,
    images: [
      {
        url: `${CANONICAL}/og.png`,
        alt: "SafeSelect MCP: read-only PostgreSQL and MongoDB for coding agents",
      },
    ],
  },
  twitter: {
    card: "summary_large_image",
    title: homeTitle,
    description: homeDescription,
    images: [`${CANONICAL}/og.png`],
  },
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
