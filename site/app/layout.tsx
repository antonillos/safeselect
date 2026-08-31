import type { Metadata } from "next";
import { CANONICAL, sitePath } from "./shared";
import { homeTitle, homeDescription } from "./metadata";
import "./globals.css";

export const metadata: Metadata = {
  metadataBase: new URL(`${CANONICAL}/`),
  title: homeTitle,
  description: homeDescription,
  icons: {
    icon: [
      { url: sitePath("/icon.svg"), type: "image/svg+xml" },
      { url: sitePath("/favicon-32.png"), type: "image/png", sizes: "32x32" },
    ],
    apple: [{ url: sitePath("/apple-touch-icon.png"), sizes: "180x180" }],
  },
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
