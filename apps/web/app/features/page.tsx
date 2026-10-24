import type { Metadata } from "next";
import Link from "next/link";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { CURRENT_VERSION } from "@/lib/site";

export const metadata: Metadata = {
  title: "Features - W0rkTree",
  description: `What W0rkTree ${CURRENT_VERSION} does today: snapshots, nested trees, staged visibility, a multi-tenant server, branch protection, license rules and Git interop.`,
};

interface FeatureItem {
  title: string;
  description: string;
}

interface FeatureSection {
  id: string;
  badge: string;
  title: string;
  intro?: string;
  items: FeatureItem[];
  command?: string;
  learnMore: { label: string; href: string };
}

const sections: FeatureSection[] = [
  {
    id: "staged-visibility",
    badge: "Real-Time Collaboration",
    title: "See what your team is working on — before they push.",
    intro:
      "Git offers no visibility into in-flight work. W0rkTree lets each developer publish a staged snapshot of their work in progress that teammates can list, without it touching branch history.",
    items: [
      {
        title: "Staged Snapshots",
        description:
          "One per tree, account and branch. A newer stage replaces the older one; pushing clears it.",
      },
      {
        title: "Team Status",
        description:
          "wt status --team lists who has staged changes, on which branch, and which files they touched.",
      },
      {
        title: "Live Events",
        description:
          "The server publishes snapshot.staged and staged.cleared events on its SSE stream and in the admin panel.",
      },
    ],
    command: "wt stage && wt status --team",
    learnMore: { label: "Staged visibility guide", href: "/guides/staged-visibility" },
  },
  {
    id: "trees",
    badge: "Core",
    title: "Nested Trees",
    intro:
      "A worktree has a root tree and any number of nested trees, each marked by a .wt-tree/ directory.",
    items: [
      {
        title: "Tree Isolation",
        description:
          "Each tree has its own branches, HEAD, reflog and history. A tree's snapshots exclude nested trees' paths.",
      },
      {
        title: "Snapshots & History",
        description:
          "BLAKE3 content-addressed blobs and manifests; snapshots are immutable and form an append-only graph.",
      },
      {
        title: "Targeted Commands",
        description:
          "Commands act on the tree containing the current directory, or on any tree with the global --tree flag.",
      },
    ],
    command: "wt tree add frontend frontend",
    learnMore: { label: "Nested trees guide", href: "/guides/nested-trees" },
  },
  {
    id: "branches",
    badge: "Workflow",
    title: "Branches, Merging & Protection",
    items: [
      {
        title: "Per-Tree Branches",
        description:
          "Create, switch, archive and restore branches. Archived branches keep their history and can be restored.",
      },
      {
        title: "Three-Way Merge",
        description:
          "Fast-forward when possible, merge snapshots otherwise. Conflicts get markers and exit code 3.",
      },
      {
        title: "Branch Protection",
        description:
          "Per-pattern rules: no direct push, required approvals, allowed pushers, no delete.",
      },
      {
        title: "Merge Requests",
        description:
          "Open, approve and merge on the server through the REST API or the admin panel.",
      },
    ],
    command: "wt merge feature/login",
    learnMore: { label: "Branch protection guide", href: "/guides/branch-protection" },
  },
  {
    id: "iam",
    badge: "Access Control",
    title: "Built-in identity, access control and multi-tenancy.",
    intro:
      "Git has no identity or access control of its own. The W0rkTree server authenticates every gRPC and REST call and authorizes it against tenant roles and tree visibility.",
    items: [
      {
        title: "Tenant Model",
        description:
          "Every account has a personal tenant; organizations are tenants with members.",
      },
      {
        title: "5 Built-in Roles",
        description: "Owner → Admin → Maintainer → Developer → Viewer.",
      },
      {
        title: "Tree Visibility",
        description:
          "Private (tenant members), internal (any signed-in account) or public (anonymous reads).",
      },
      {
        title: "Tokens",
        description:
          "Session tokens from login plus named personal API tokens with optional expiry for CI and scripts.",
      },
    ],
    command: "wt clone http://localhost:7420 acme/app",
    learnMore: { label: "Multi-tenancy guide", href: "/guides/multi-tenancy" },
  },
  {
    id: "licensing",
    badge: "Governance",
    title: "Per-path license compliance, checked by the server.",
    items: [
      {
        title: "SPDX Rules",
        description:
          "Attach an SPDX license identifier to any path glob in a tree.",
      },
      {
        title: "License Grants",
        description:
          "Each rule grants other tenants read-only, modify or redistribute rights.",
      },
      {
        title: "Push Enforcement",
        description:
          "Pushes that change paths the pusher's tenant may not modify are rejected with a license violation.",
      },
    ],
    command: "PUT /api/v1/trees/{tenant}/{tree}/licenses",
    learnMore: { label: "License compliance guide", href: "/guides/license-compliance" },
  },
  {
    id: "automation",
    badge: "Automatic by Default",
    title: "A background daemon that handles the tedium.",
    items: [
      {
        title: "File Watching",
        description:
          "worktree-bg watches registered worktrees and ignores everything under .wt/.",
      },
      {
        title: "Auto-Snapshots",
        description:
          "After 30 s of inactivity or 50 changed files (configurable), it snapshots each changed tree.",
      },
      {
        title: "Auto-Stage",
        description:
          "With a remote and saved credentials, each auto-snapshot is staged for the team.",
      },
      {
        title: "Merge-Safe",
        description:
          "The daemon never snapshots while a merge is in progress, and can be paused at any time.",
      },
    ],
    command: "wt daemon start && wt daemon watch .",
    learnMore: { label: "Auto-tracking guide", href: "/guides/auto-tracking-setup" },
  },
  {
    id: "sync",
    badge: "Sync Protocol",
    title: "Explicit push, pull and stage over one port.",
    items: [
      {
        title: "Push",
        description:
          "Uploads only the objects the server lacks, then moves the branch tip if it still descends from the server's tip.",
      },
      {
        title: "Pull & Fetch",
        description:
          "Downloads missing snapshots and objects, then fast-forwards or merges into your branch.",
      },
      {
        title: "Stage",
        description:
          "Shares your working state as a staged snapshot without changing any branch.",
      },
    ],
    command: "wt pull && wt push",
    learnMore: { label: "Sync protocol reference", href: "/docs/protocol/sync-protocol" },
  },
  {
    id: "config",
    badge: "Configuration",
    title: "Plain TOML, discoverable with wt config.",
    items: [
      {
        title: ".wt/config.toml",
        description:
          "Worktree identity, user, auto-snapshot thresholds, sync settings and named remotes.",
      },
      {
        title: "$WT_HOME",
        description:
          "Per-user credentials, daemon state and the daemon's watch list, outside any repository.",
      },
    ],
    command: "wt config set user.email alice@example.com",
    learnMore: { label: "Configuration model", href: "/guides/config-model" },
  },
  {
    id: "safety",
    badge: "Non-Destructive by Design",
    title: "No rebase. No force push. No lost work.",
    items: [
      {
        title: "Append-Only History",
        description: "Snapshots are immutable locally and on the server.",
      },
      {
        title: "Reflog",
        description: "Every operation that moves a branch is recorded per tree.",
      },
      {
        title: "Revert & Restore",
        description:
          "Revert adds an inverse snapshot; restore rewrites only working files.",
      },
    ],
    command: "wt revert HEAD",
    learnMore: { label: "Recovery guide", href: "/guides/reflog-and-recovery" },
  },
  {
    id: "security",
    badge: "Security",
    title: "Secrets stay out of history.",
    items: [
      {
        title: "Secret Scanning",
        description:
          "Added and modified files are scanned before every snapshot; findings block the snapshot with redacted excerpts.",
      },
      {
        title: "Credential Storage",
        description:
          "Passwords are hashed with argon2id; API tokens are stored only as SHA-256 hashes.",
      },
      {
        title: "Audit Log",
        description:
          "Pushes, logins and administrative changes are recorded in an append-only audit log.",
      },
    ],
    learnMore: { label: "Security model", href: "/docs/security" },
  },
  {
    id: "git",
    badge: "Migration",
    title: "Git Compatibility",
    items: [
      {
        title: "Import",
        description: "Convert a Git repository's history into W0rkTree snapshots.",
      },
      {
        title: "Export",
        description:
          "Write W0rkTree snapshots back into a Git repository for Git-only tooling.",
      },
    ],
    command: "wt git import ../legacy-repo ./app",
    learnMore: { label: "Migration guide", href: "/guides/migration" },
  },
  {
    id: "admin",
    badge: "Operations",
    title: "An admin panel and API for every server object.",
    items: [
      {
        title: "Admin Panel",
        description:
          "Dashboard with live activity, tenants, trees, history, file browser, merge requests and audit log.",
      },
      {
        title: "REST API",
        description:
          "JSON API under /api/v1 with bearer-token auth and consistent error codes.",
      },
      {
        title: "Monitoring",
        description: "/healthz for liveness and /metrics in Prometheus format.",
      },
    ],
    learnMore: { label: "Admin panel guide", href: "/guides/admin-panel" },
  },
];

export default function FeaturesPage() {
  return (
    <>
      <section className="border-b bg-muted/30 py-24">
        <div className="container mx-auto max-w-4xl px-4 text-center">
          <Badge variant="outline" className="mb-6">
            {CURRENT_VERSION}
          </Badge>
          <h1 className="mb-6 text-5xl font-bold tracking-tight">
            Version control, rebuilt from the protocol up.
          </h1>
          <p className="mb-10 text-xl text-muted-foreground">
            Not a Git wrapper or extension: a complete system with its own object
            model, sync protocol and server, plus Git import and export as a
            migration bridge.
          </p>
          <div className="flex justify-center gap-4">
            <Button size="lg" asChild>
              <Link href="/guides/quick-start">Get Started</Link>
            </Button>
            <Button size="lg" variant="outline" asChild>
              <Link href="/docs/protocol">Read the Protocol</Link>
            </Button>
          </div>
        </div>
      </section>

      {sections.map((section, index) => (
        <section
          key={section.id}
          id={section.id}
          aria-labelledby={`${section.id}-heading`}
          className={`scroll-mt-20 border-b py-20 ${index % 2 === 1 ? "bg-muted/10" : ""}`}
        >
          <div className="container mx-auto max-w-5xl px-4">
            <Badge className="mb-4" variant={index % 2 === 1 ? "outline" : "default"}>
              {section.badge}
            </Badge>
            <h2 id={`${section.id}-heading`} className="mb-4 text-3xl font-bold">
              {section.title}
            </h2>
            {section.intro && (
              <p className="mb-10 text-muted-foreground">{section.intro}</p>
            )}
            <div
              className={`mt-6 grid gap-6 md:grid-cols-2 ${
                section.items.length >= 4 ? "lg:grid-cols-4" : section.items.length === 3 ? "lg:grid-cols-3" : ""
              }`}
            >
              {section.items.map((item) => (
                <div key={item.title} className="rounded-xl border bg-card p-6 shadow-sm">
                  <h3 className="mb-2 font-bold">{item.title}</h3>
                  <p className="text-sm text-muted-foreground">{item.description}</p>
                </div>
              ))}
            </div>
            <div className="mt-8 flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
              {section.command && (
                <code className="rounded-lg bg-muted p-4 font-mono text-sm">
                  {section.command}
                </code>
              )}
              <Link
                href={section.learnMore.href}
                className="text-sm font-medium text-primary underline-offset-4 hover:underline"
              >
                {section.learnMore.label} →
              </Link>
            </div>
          </div>
        </section>
      ))}

      <section aria-labelledby="roadmap-heading" className="border-b py-20">
        <div className="container mx-auto max-w-5xl px-4">
          <Badge variant="outline" className="mb-4">
            Roadmap
          </Badge>
          <h2 id="roadmap-heading" className="mb-4 text-3xl font-bold">
            Not in {CURRENT_VERSION} yet
          </h2>
          <p className="mb-6 text-muted-foreground">
            QUIC transport, virtual-filesystem lazy loading, chunked large-file
            transfer, CI gates, linked branches across trees, declarative access
            policies, snapshot signing and prebuilt binaries are planned but not
            shipped. They are tracked on the roadmap.
          </p>
          <Link
            href="/roadmap"
            className="text-sm font-medium text-primary underline-offset-4 hover:underline"
          >
            View the roadmap →
          </Link>
        </div>
      </section>

      <section className="py-20">
        <div className="container mx-auto max-w-4xl px-4 text-center">
          <h2 className="mb-8 text-4xl font-bold">Ready to evolve your workflow?</h2>
          <div className="flex justify-center gap-4">
            <Button size="lg" asChild>
              <Link href="/guides/quick-start">Get Started</Link>
            </Button>
            <Button size="lg" variant="outline" asChild>
              <Link href="/docs/cli">CLI Reference</Link>
            </Button>
          </div>
        </div>
      </section>
    </>
  );
}
