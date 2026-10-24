import type { Metadata } from "next";
import { GeistMono } from "geist/font/mono";
import "./globals.css";
import { RootProvider } from "fumadocs-ui/provider/next";

export const metadata: Metadata = {
  title: "W0rkTree - Next-Generation Version Control",
  description:
    "Version control with nested trees, automatic change tracking, team-visible work in progress and a self-hosted multi-tenant server. Rust CLI, Go server, Git import and export.",
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
  authors: [{ name: "W0rkTree Team" }],
  openGraph: {
    title: "W0rkTree - Next-Generation Version Control",
    description:
      "Version control with nested trees, automatic change tracking, team-visible work in progress and Git import and export.",
    type: "website",
  },
  twitter: {
    card: "summary_large_image",
    title: "W0rkTree - Next-Generation Version Control",
    description:
      "Version control with nested trees, automatic change tracking, team-visible work in progress and Git import and export.",
  },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body className={`${GeistMono.className} flex flex-col min-h-screen`}>
        <RootProvider>{children}</RootProvider>
      </body>
    </html>
  );
}
