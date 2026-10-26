import type { Metadata } from "next";
import Link from "next/link";
import { RiArrowRightLine, RiRoadMapLine, RiStackLine } from "react-icons/ri";
import { ActionLink } from "@/components/site/action";
import { Band, gutter } from "@/components/site/frame";
import { FadeUp, StaggerChild, StaggerList } from "@/components/site/motion-text";
import { PageHero } from "@/components/site/page-hero";
import {
  CellArrow,
  HoverHatch,
  RuleItem,
  cellLink,
  hairlineGrid,
} from "@/components/features/cells";
import {
  ChangeBadge,
  changeOrder,
  type ChangeType,
} from "@/components/features/change-badge";
import { cx } from "@/utils/cx";

export const metadata: Metadata = {
  title: "Changelog",
  description: "Release history and updates for W0rkTree.",
};

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
    version: "v0.8.0",
    date: "2026",
    title: "Scale, governance and safety nets",
    description:
      "Pack and delta transfer, forks, config-as-code, the tenant lifecycle, ABAC attributes, device sign-in, server-side secret scanning, labeled staged snapshots, browser conflict resolution, force push and undo, and server operations.",
    changes: [
      {
        type: "Added",
        text: "Pack files with binary deltas for push, fetch and clone (resumable uploads; wt push --no-pack to opt out). Small-edit pushes send hundreds of times fewer bytes.",
      },
      {
        type: "Added",
        text: "Forks across tenants (wt fork), fork sync and merge requests from forks (wt mr create --from).",
      },
      {
        type: "Added",
        text: "Config-as-code: .wt-tree/branches.toml and protections.toml validated on push with located errors and applied after it; wt trees config, wt branch describe/owner.",
      },
      {
        type: "Added",
        text: "Tenant email verification, slug renames with reservations, deletion with a 30-day recovery window and restore; wt tenant switch.",
      },
      {
        type: "Added",
        text: "ABAC attributes on tenants, accounts and trees with attribute conditions in policies and the simulator (wt attr).",
      },
      {
        type: "Added",
        text: "Device sign-in (wt login --device) and rotating refresh tokens with reuse detection.",
      },
      {
        type: "Added",
        text: "Server-side secret scanning on push (off, warn or block, allowlists) and a license audit with strict SPDX checks (wt compliance, wt license audit).",
      },
      {
        type: "Added",
        text: "Labeled staged snapshots with history and expiry, wt checkout --staged and overlap notices on push.",
      },
      {
        type: "Added",
        text: "Conflict resolution in the admin panel and wt mr resolve; rebase merges and deleting the source branch on merge.",
      },
      {
        type: "Added",
        text: "Force push with leases (branch:force_push, no_force_push protections), audited protection bypass, wt undo/redo and wt tag delete --remote.",
      },
      {
        type: "Added",
        text: "Backups to S3, a durable email outbox (wt admin outbox) and retention for policy versions, audit events and webhook deliveries.",
      },
      {
        type: "Breaking",
        text: "Logins return a one-hour access token plus a refresh token; clients that ignore refresh_token must sign in again after an hour. wt tenant rename now changes the slug (the display name moved to wt tenant set-name).",
      },
      {
        type: "Changed",
        text: "wt mirror takes the tree with --in (the old --tree still works with a warning); staged snapshots expire after 30 days by default.",
      },
    ],
  },
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

/** A URL fragment for a release ("v0.3.0–v0.4.0" becomes "v0-3-0-v0-4-0"). */
function anchor(version: string) {
  return version.toLowerCase().replace(/[^a-z0-9]+/g, "-");
}

function groupChanges(changes: Change[]) {
  return changeOrder
    .map((type) => ({ type, items: changes.filter((change) => change.type === type) }))
    .filter((group) => group.items.length > 0);
}

function plural(count: number, word: string) {
  return `${count} ${word}${count === 1 ? "" : "s"}`;
}

export default function ChangelogPage() {
  const latest = releases[0]!;
  return (
    <>
      <PageHero
        eyebrow="Changelog"
        title="Every release,"
        muted="tracked in one place."
        actions={
          <>
            <ActionLink href={`#${anchor(latest.version)}`} trailingIcon={RiArrowRightLine}>
              Latest: {latest.version}
            </ActionLink>
            <ActionLink href="/roadmap" variant="secondary" leadingIcon={RiRoadMapLine}>
              Roadmap
            </ActionLink>
          </>
        }
        meta={`${plural(releases.length, "release")} · ${plural(
          releases.reduce((total, release) => total + release.changes.length, 0),
          "change",
        )}`}
      >
        Planned work lives on the{" "}
        <Link
          href="/roadmap"
          className="text-text-primary underline decoration-text-tertiary underline-offset-4 transition-colors hover:decoration-text-primary"
        >
          roadmap
        </Link>
        .
      </PageHero>

      <ReleaseIndex />

      <div>
        {releases.map((release, index) => (
          <ReleaseBand
            key={release.version}
            release={release}
            latest={index === 0}
            sunken={index % 2 === 1}
          />
        ))}
      </div>

      <Band marks={false}>
        <div
          className={cx(
            "relative flex flex-col items-start justify-between gap-6 overflow-hidden py-12 md:flex-row md:items-center",
            gutter,
          )}
        >
          <div aria-hidden className="hatch pointer-events-none absolute inset-0 opacity-50" />
          <div className="relative flex flex-col gap-2">
            <p className="eyebrow">What comes next</p>
            <p className="text-title-2-semibold tracking-[-0.02em]">
              Planned work lives on the roadmap.
            </p>
          </div>
          <div className="relative flex flex-wrap gap-2.5">
            <ActionLink href="/roadmap" trailingIcon={RiArrowRightLine}>
              View the roadmap
            </ActionLink>
            <ActionLink href="/features" variant="secondary" leadingIcon={RiStackLine}>
              Features
            </ActionLink>
          </div>
        </div>
      </Band>
    </>
  );
}

/** Every release at a glance: version, title, and what kinds of change it carries. */
function ReleaseIndex() {
  const columns = 4;
  const filler = (columns - (releases.length % columns)) % columns;
  return (
    <Band>
      <StaggerList className={cx(hairlineGrid, "sm:grid-cols-2 lg:grid-cols-4")}>
        {releases.map((release, index) => (
          <li key={release.version} className="bg-background-full">
            <StaggerChild className="h-full">
              <a href={`#${anchor(release.version)}`} className={cx(cellLink, "gap-3 sm:p-6")}>
                <HoverHatch />
                <span className="relative flex items-center gap-2">
                  {index === 0 ? <span aria-hidden className="live-dot" /> : null}
                  <span className="font-mono text-body-2-medium tabular-nums">
                    {release.version}
                  </span>
                  <span className="text-caption-1-regular text-text-tertiary">{release.date}</span>
                  <CellArrow className="ms-auto" />
                </span>
                <span className="relative text-body-medium text-pretty">{release.title}</span>
                <span className="relative mt-auto flex flex-wrap gap-1.5 pt-1">
                  {groupChanges(release.changes).map((group) => (
                    <span key={group.type} className="inline-flex items-center gap-1">
                      <ChangeBadge type={group.type} />
                      <span className="font-mono text-caption-2-regular text-text-secondary tabular-nums">
                        {group.items.length}
                      </span>
                    </span>
                  ))}
                </span>
              </a>
            </StaggerChild>
          </li>
        ))}
        {Array.from({ length: filler }, (_, index) => (
          <li
            key={`filler-${index}`}
            aria-hidden
            className="hatch hidden bg-background-full lg:block"
          />
        ))}
      </StaggerList>
    </Band>
  );
}

function ReleaseBand({
  release,
  latest,
  sunken,
}: {
  release: Release;
  latest: boolean;
  sunken: boolean;
}) {
  const id = anchor(release.version);
  const groups = groupChanges(release.changes);
  return (
    <Band
      id={id}
      aria-labelledby={`${id}-title`}
      frameClassName={cx(sunken && "bg-surface-sunken")}
    >
      {/* The release's node on the left rail: the rails double as the timeline. */}
      <span
        aria-hidden
        className={cx(
          "absolute top-[4.6rem] left-0 z-10 hidden size-[9px] -translate-x-1/2 rotate-45 border border-text-primary sm:block md:top-[5.85rem]",
          latest ? "bg-text-primary" : "bg-background-full",
        )}
      />
      <div className={cx("grid gap-10 py-14 md:py-20 lg:grid-cols-12", gutter)}>
        <header className="flex flex-col gap-4 lg:sticky lg:top-24 lg:col-span-4 lg:self-start">
          <p className="flex items-center gap-3">
            <span className="font-mono text-display-4-medium tracking-[-0.03em] tabular-nums">
              {release.version}
            </span>
            {latest ? (
              <span className="inline-flex h-6 items-center gap-1.5 rounded-full border border-border-button-default bg-background-full px-2 text-caption-1-medium">
                <span aria-hidden className="live-dot" />
                Latest
              </span>
            ) : null}
          </p>
          <p className="eyebrow">{release.date}</p>
          <h2
            id={`${id}-title`}
            className="text-title-2-semibold tracking-[-0.02em] text-balance"
          >
            {release.title}
          </h2>
          <p className="text-body-regular leading-6 text-pretty text-text-secondary">
            {release.description}
          </p>
          <p className="font-mono text-caption-1-regular text-text-tertiary">
            {plural(release.changes.length, "change")}
          </p>
        </header>

        <div className="flex min-w-0 flex-col gap-10 lg:col-span-8">
          {groups.map((group) => (
            <FadeUp key={group.type}>
              <section aria-label={`${group.type} in ${release.version}`}>
                <div className="flex items-center gap-3">
                  <ChangeBadge type={group.type} />
                  <span className="font-mono text-caption-1-regular text-text-tertiary tabular-nums">
                    {group.items.length}
                  </span>
                  <span aria-hidden className="h-px flex-1 bg-separator-border" />
                </div>
                <ul className="mt-4 flex flex-col gap-3">
                  {group.items.map((change) => (
                    <RuleItem key={change.text} className="text-text-primary">
                      {change.text}
                    </RuleItem>
                  ))}
                </ul>
              </section>
            </FadeUp>
          ))}
        </div>
      </div>
    </Band>
  );
}
