import type { Metadata, Viewport } from "next";
import { Inter, JetBrains_Mono } from "next/font/google";
import { RootProvider } from "fumadocs-ui/provider/next";
import { appName, creator, siteUrl } from "@/lib/shared";
import "./globals.css";

// BoardUI's type ramp is drawn for Inter; its code face is JetBrains Mono (styles/theme.css reads both variables).
const sans = Inter({ subsets: ["latin"], variable: "--font-inter" });
const mono = JetBrains_Mono({
  subsets: ["latin"],
  variable: "--font-mono-source",
});

const title = "W0rkTree: version control with nested trees";
const description =
  "Version control with nested trees, automatic change tracking, team-visible work in progress and a self-hosted multi-tenant server. Rust CLI, Go server, Git import and export.";

export const metadata: Metadata = {
  metadataBase: new URL(siteUrl),
  title: { template: `%s | ${appName}`, default: title },
  description,
  applicationName: appName,
  keywords: [
    "version control",
    "git",
    "vcs",
    "rust",
    "nested trees",
    "automatic tracking",
    "worktree",
    "w0rktree",
  ],
  authors: [{ name: creator }],
  creator,
  // Version numbers and ports in code samples are not phone numbers.
  formatDetection: { telephone: false, email: false, address: false },
  openGraph: { title, description, type: "website", siteName: appName },
  twitter: { card: "summary_large_image", title, description },
  icons: { icon: [{ url: "/icon.svg", type: "image/svg+xml" }] },
};

export const viewport: Viewport = {
  themeColor: [
    { media: "(prefers-color-scheme: light)", color: "#ffffff" },
    { media: "(prefers-color-scheme: dark)", color: "#121212" },
  ],
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html
      lang="en"
      className={`${sans.variable} ${mono.variable}`}
      suppressHydrationWarning
    >
      <body className="flex min-h-screen flex-col font-sans antialiased">
        <RootProvider>{children}</RootProvider>
      </body>
    </html>
  );
}
