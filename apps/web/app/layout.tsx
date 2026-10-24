import type { Metadata } from "next";
import { GeistMono } from "geist/font/mono";
import "./globals.css";
import { RootProvider } from "fumadocs-ui/provider/next";

export const metadata: Metadata = {
  title: "W0rkTree - Next-Generation Version Control",
  description:
    "Modern version control system with nested trees, automatic change tracking, fine-grained permissions, and full Git compatibility. Built in Rust for speed and reliability.",
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
      "Modern version control system with nested trees, automatic change tracking, and full Git compatibility.",
    type: "website",
  },
  twitter: {
    card: "summary_large_image",
    title: "W0rkTree - Next-Generation Version Control",
    description:
      "Modern version control system with nested trees, automatic change tracking, and full Git compatibility.",
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
