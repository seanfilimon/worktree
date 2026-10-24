import type { Metadata } from "next";
import Link from "next/link";

export const metadata: Metadata = {
  title: "About - W0rkTree",
  description: "Why W0rkTree exists and how it is built.",
};

const principles = [
  {
    title: "Client and server, each in its best language",
    text: "The wt CLI, SDK and background daemon are Rust; the multi-tenant server is a single Go binary that serves gRPC sync, REST, live events and the admin panel on one port.",
  },
  {
    title: "Staged visibility",
    text: "Work in progress can be shared with the team as staged snapshots, without touching branch history.",
  },
  {
    title: "Multi-tenancy built in",
    text: "Accounts, organizations, roles and tree visibility are part of the server, not an add-on.",
  },
  {
    title: "Governance at the server",
    text: "Branch protection, merge requests and per-path SPDX license rules are enforced on every push.",
  },
  {
    title: "Nothing is rewritten",
    text: "History is append-only: reverts add snapshots, branches are archived, and there is no force push.",
  },
];

export default function AboutPage() {
  return (
    <div className="container mx-auto max-w-4xl px-4 py-20">
      <h1 className="mb-6 text-4xl font-bold">About W0rkTree</h1>
      <p className="mb-8 text-lg text-muted-foreground">
        W0rkTree is not a Git wrapper. It is a version control system with its
        own object model, sync protocol and server, built to address the
        collaboration and access-control gaps teams work around in Git today.
        Git import and export make it possible to move over gradually.
      </p>
      <h2 className="mb-4 text-2xl font-semibold">Technical Philosophy</h2>
      <ul className="mb-8 list-disc space-y-2 pl-6 text-muted-foreground">
        {principles.map((principle) => (
          <li key={principle.title}>
            <strong className="text-foreground">{principle.title}:</strong> {principle.text}
          </li>
        ))}
      </ul>
      <p className="text-muted-foreground">
        Read the{" "}
        <Link href="/guides/architecture" className="underline underline-offset-4 hover:text-foreground">
          architecture overview
        </Link>{" "}
        or see what is planned on the{" "}
        <Link href="/roadmap" className="underline underline-offset-4 hover:text-foreground">
          roadmap
        </Link>
        .
      </p>
    </div>
  );
}
