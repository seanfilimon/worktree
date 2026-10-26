import type { Metadata } from "next";
import Link from "next/link";
import { Badge } from "@/components/ui/badge";

export const metadata: Metadata = {
  title: "Changelog - W0rkTree",
  description: "Release history and updates for W0rkTree.",
};

type ChangeType = "Added" | "Changed" | "Fixed" | "Breaking";

interface Change {
  type: ChangeType;
  text: string;
}

interface Release {
  version: string;
  date: string;
  title: string;
  description: string;
  changes: Change[];
}

const releases: Release[] = [
  {
    version: "v0.7.0",
    date: "2026",
    title: "Deployable and team-ready",
    description:
      "Packaging, pluggable storage and backups, notifications, sparse checkout, one-step sync, Git mirrors, a deeper review workflow, release integrity and code search v2.",
    changes: [
      {
        type: "Added",
        text: "Docker images (deploy/docker), Compose profiles for S3 and SMTP, a Helm chart, systemd and launchd units, install scripts and release archives with SHA256SUMS.",
      },
      {
        type: "Added",
        text: "S3-compatible object store (--object-store s3), scheduled .wtbak backups, offline worktree-server restore and wt admin backup.",
      },
      {
        type: "Added",
        text: "Notifications: inbox for review requests, mentions, CI failures, TODOs and merges; watch levels; optional SMTP email; wt inbox.",
      },
      {
        type: "Added",
        text: "wt sparse and partial clones (wt clone --sparse) with on-demand downloads; wt sync and the offline push queue.",
      },
      {
        type: "Added",
        text: "Git push mirrors with license filtering (wt mirror); path owners and OWNERS files, review requests, draft merge requests, suggested changes.",
      },
      {
        type: "Added",
        text: "Signed tags (wt tag --sign, wt tag verify), release checksums, SPDX SBOMs and wt release verify.",
      },
      {
        type: "Changed",
        text: "Code search keeps a persistent index on disk, adds symbol search and branch search.",
      },
      {
        type: "Changed",
        text: "A tree's .wt-tree/ config, ignore, hooks and access files are shared through snapshots; the name/id marker moved to .wt-tree/local.toml.",
      },
      {
        type: "Fixed",
        text: "Faster pushes into large trees, parallel object transfer and cached policy evaluation.",
      },
    ],
  },
  {
    version: "v0.6.0",
    date: "2026",
    title: "Collaboration and tooling",
    description:
      "Server-side merges, webhooks, code search, blame, history rewriting, compressed storage, policy history and CLI ergonomics.",
    changes: [
      {
        type: "Added",
        text: "Merge requests merge on the server by fast-forward, merge snapshot or squash, with conflict reporting and wt mr mergeability.",
      },
      {
        type: "Added",
        text: "Signed webhooks with retries and a delivery log; code search; wt blame; wt cherry-pick, wt rebase and wt stash.",
      },
      {
        type: "Added",
        text: "zstd-compressed objects, gRPC gzip, wt gc, wt fsck and server fsck; policy versions, diffs and rollback.",
      },
      {
        type: "Added",
        text: "wt completions, wt doctor, command aliases, wt open, an OpenAPI 3.1 document at /api/v1/openapi.yaml and wt api.",
      },
    ],
  },
  {
    version: "v0.5.0",
    date: "2026",
    title: "Protocol completeness",
    description:
      "Every protocol spec rewritten to match the code, with the missing features built.",
    changes: [
      {
        type: "Added",
        text: "Chunked large files, shallow sync, path-scoped access and license-restricted paths.",
      },
      {
        type: "Added",
        text: "CI status checks, stale approvals, review comments, linked merge groups and tag protection.",
      },
      {
        type: "Added",
        text: "Snapshot dependencies and TODOs, releases with assets, Ed25519 snapshot signing, declarative access files.",
      },
      {
        type: "Added",
        text: "TLS, garbage collection, quotas, rate limiting, audit export, the daemon sync loop and a Git bridge to remotes.",
      },
    ],
  },
  {
    version: "v0.3.0–v0.4.0",
    date: "2026",
    title: "Admin panel and IAM",
    description:
      "A full monochrome admin panel and a complete identity and access system.",
    changes: [
      {
        type: "Added",
        text: "Teams, custom roles, policies with conditions, tree grants, service accounts, scoped tokens, TOTP, sessions, invitations and OpenID Connect SSO.",
      },
      { type: "Added", text: "A wt command group for every server feature." },
    ],
  },
  {
    version: "v0.2.0",
    date: "2026",
    title: "The v2 Rebuild",
    description:
      "A ground-up rebuild of W0rkTree around a single binding contract: a Rust CLI, SDK and background daemon on the client, a Go server that serves gRPC sync, REST, live events and the admin panel on one port, and a new React admin panel.",
    changes: [
      {
        type: "Breaking",
        text: "The Rust worktree-server crate is removed; the server is now the Go service in services/server-go, listening on a single port (default 7420).",
      },
      {
        type: "Breaking",
        text: "The Yew admin crate is replaced by apps/admin (Vite + React 19), served by the server at /admin/.",
      },
      {
        type: "Breaking",
        text: "New local repository layout under .wt/ and a new wire contract, worktree.sync.v1. Repositories from v0.1.0 must be re-created or re-imported.",
      },
      {
        type: "Added",
        text: "wt CLI: init, status (--team), snapshot, log, show, diff, branch, switch, merge, revert, restore, tag, tree, reflog, ignore, config, remote, register, login, logout, whoami, push, fetch, pull, clone, stage, archive, Git import/export and daemon.",
      },
      {
        type: "Added",
        text: "Global --json, -C, --tree and --no-color flags with stable exit codes (0 ok, 1 error, 2 usage, 3 merge conflicts).",
      },
      {
        type: "Added",
        text: "worktree-bg daemon: file watching, debounced auto-snapshots and automatic staging of work in progress.",
      },
      {
        type: "Added",
        text: "Staged snapshots: team-visible work in progress, listed with wt status --team and streamed as server events.",
      },
      {
        type: "Added",
        text: "Server: tenants and memberships with five built-in roles, private/internal/public trees, session and personal API tokens, argon2id password hashing.",
      },
      {
        type: "Added",
        text: "Server: branch protection, merge requests, per-path SPDX license rules, append-only audit log, SSE event stream, /healthz and Prometheus /metrics.",
      },
      {
        type: "Added",
        text: "Secret scanning on added and modified files before every snapshot.",
      },
      {
        type: "Changed",
        text: "Sync uses gRPC over cleartext HTTP/2 (h2c) with object negotiation, so push and pull transfer only missing objects. QUIC remains on the roadmap.",
      },
      {
        type: "Changed",
        text: "Website and documentation rewritten to match the v0.2.0 implementation; features that are not built yet are marked as roadmap items.",
      },
    ],
  },
  {
    version: "v0.1.0",
    date: "2025",
    title: "Initial Prototype",
    description:
      "The first prototype of W0rkTree: the nested tree model, BLAKE3 content-addressed storage, snapshots and branches, and early protocol specification drafts.",
    changes: [
      {
        type: "Added",
        text: "Nested tree model and BLAKE3 content-addressed object storage.",
      },
      {
        type: "Added",
        text: "Snapshot creation and basic branching.",
      },
      {
        type: "Added",
        text: "Protocol specification drafts, an initial Rust server and a Yew admin panel.",
      },
    ],
  },
];

const badgeVariant: Record<
  ChangeType,
  "secondary" | "outline" | "default" | "destructive"
> = {
  Added: "secondary",
  Changed: "outline",
  Fixed: "default",
  Breaking: "destructive",
};

export default function ChangelogPage() {
  return (
    <div className="container mx-auto max-w-4xl px-6 py-16 md:py-24">
      <section className="mb-16 text-center">
        <h1 className="text-4xl font-bold tracking-tight sm:text-5xl">
          Changelog
        </h1>
        <p className="mt-4 text-lg text-muted-foreground">
          Every release, tracked in one place. Planned work lives on the{" "}
          <Link
            href="/roadmap"
            className="underline underline-offset-4 hover:text-foreground"
          >
            roadmap
          </Link>
          .
        </p>
      </section>

      <section className="relative space-y-12" aria-label="Releases">
        <div
          className="absolute bottom-0 left-[7px] top-2 w-px bg-border"
          aria-hidden="true"
        />

        {releases.map((release) => (
          <article key={release.version} className="relative pl-10">
            <div
              className="absolute left-0 top-2 h-[15px] w-[15px] rounded-full border-2 border-primary bg-background"
              aria-hidden="true"
            />

            <div className="flex flex-wrap items-center gap-3">
              <Badge>{release.version}</Badge>
              <span className="text-sm text-muted-foreground">
                {release.date}
              </span>
            </div>

            <h2 className="mt-3 text-2xl font-semibold tracking-tight">
              {release.title}
            </h2>

            <p className="mt-2 leading-relaxed text-muted-foreground">
              {release.description}
            </p>

            <ul className="mt-5 space-y-3">
              {release.changes.map((change) => (
                <li key={change.text} className="flex items-start gap-3">
                  <Badge
                    variant={badgeVariant[change.type]}
                    className="mt-0.5 min-w-[68px] shrink-0 justify-center text-[11px]"
                  >
                    {change.type}
                  </Badge>
                  <span className="text-sm leading-relaxed text-foreground/90">
                    {change.text}
                  </span>
                </li>
              ))}
            </ul>
          </article>
        ))}
      </section>
    </div>
  );
}
