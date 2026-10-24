import type { Metadata } from "next";
import Link from "next/link";
import { Badge } from "@/components/ui/badge";
import { CURRENT_VERSION, ISSUES_URL } from "@/lib/site";

export const metadata: Metadata = {
  title: "Roadmap - W0rkTree",
  description:
    "What shipped in the W0rkTree v0.2.0 rebuild and what is planned next.",
};

interface Milestone {
  title: string;
  status: "shipped" | "planned";
  summary: string;
  items: string[];
}

const milestones: Milestone[] = [
  {
    title: `${CURRENT_VERSION} — v2 rebuild`,
    status: "shipped",
    summary:
      "A ground-up rebuild around one binding contract: a Rust client stack, a Go server on a single port and a React admin panel.",
    items: [
      "wt CLI covering init, snapshots, history, diff, branches, merge, tags, nested trees, ignore rules and config",
      "worktree-bg daemon with file watching, auto-snapshots and auto-staging",
      "Go server: gRPC sync (h2c) + REST + SSE + admin panel on port 7420",
      "Tenants, five built-in roles, tree visibility, session and API tokens",
      "Staged snapshots and wt status --team",
      "Branch protection, merge requests, per-path SPDX license rules, audit log",
      "Secret scanning before every snapshot",
      "Git import and export",
    ],
  },
  {
    title: "Distribution",
    status: "planned",
    summary: "Make W0rkTree installable without a Rust and Go toolchain.",
    items: [
      "Prebuilt binaries for Windows, macOS and Linux",
      "Package-manager installs",
      "Official server container image",
    ],
  },
  {
    title: "Transport & Storage",
    status: "planned",
    summary: "Faster sync and better handling of very large repositories.",
    items: [
      "QUIC transport alongside HTTP/2",
      "Built-in TLS termination in the server",
      "Chunked (FastCDC) large-file transfer and storage",
      "Shallow clone from the CLI",
      "FUSE / ProjFS virtual filesystem with lazy loading",
    ],
  },
  {
    title: "Governance & Automation",
    status: "planned",
    summary: "Deeper policy controls and CI integration.",
    items: [
      "CI gates and required status checks on protected branches",
      "Declarative, version-controlled access policies (RBAC/ABAC)",
      "wt subcommands for merge requests, tenants, access and licenses",
      "Rate limiting and plan quotas",
    ],
  },
  {
    title: "Multi-Tree Workflows",
    status: "planned",
    summary: "Coordinate changes that span several nested trees.",
    items: [
      "Linked branches across trees",
      "Cross-tree dependency graph",
      "Live, bidirectional Git mirroring",
    ],
  },
  {
    title: "Security Hardening",
    status: "planned",
    summary: "Stronger guarantees for regulated environments.",
    items: [
      "Snapshot signing",
      "Encryption at rest",
      "OAuth / SSO sign-in",
    ],
  },
];

export default function RoadmapPage() {
  return (
    <div className="container mx-auto max-w-4xl px-4 py-20">
      <h1 className="mb-4 text-4xl font-bold">Roadmap</h1>
      <p className="mb-10 text-lg text-muted-foreground">
        Everything under &ldquo;Planned&rdquo; is not available yet. Priorities
        can change; discussion happens on the{" "}
        <a
          href={ISSUES_URL}
          target="_blank"
          rel="noopener noreferrer"
          className="underline underline-offset-4 hover:text-foreground"
        >
          issue tracker
        </a>
        . See the{" "}
        <Link href="/changelog" className="underline underline-offset-4 hover:text-foreground">
          changelog
        </Link>{" "}
        for released work.
      </p>
      <ol className="space-y-6">
        {milestones.map((milestone) => (
          <li key={milestone.title} className="rounded-xl border bg-card p-6">
            <div className="mb-2 flex flex-wrap items-center gap-3">
              <h2 className="text-xl font-bold">{milestone.title}</h2>
              <Badge variant={milestone.status === "shipped" ? "default" : "outline"}>
                {milestone.status === "shipped" ? "Shipped" : "Planned"}
              </Badge>
            </div>
            <p className="mb-4 text-muted-foreground">{milestone.summary}</p>
            <ul className="list-disc space-y-1 pl-6 text-sm text-foreground/90">
              {milestone.items.map((item) => (
                <li key={item}>{item}</li>
              ))}
            </ul>
          </li>
        ))}
      </ol>
    </div>
  );
}
