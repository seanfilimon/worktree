import type { Metadata } from "next";
import Link from "next/link";
import {
  RiArrowRightLine,
  RiBookOpenLine,
  RiCodeSSlashLine,
  RiHistoryLine,
  RiRoadMapLine,
  RiTerminalBoxLine,
} from "react-icons/ri";
import { Reveal } from "@/components/home/motion";
import { ActionLink, TextLink } from "@/components/site/action";
import { Band, SectionHeading, gutter } from "@/components/site/frame";
import { StaggerChild, StaggerList } from "@/components/site/motion-text";
import { PageHero } from "@/components/site/page-hero";
import { ChapterNav } from "@/components/home/chapter-nav";
import {
  CellArrow,
  HoverHatch,
  IconTile,
  RuleItem,
  cellLink,
  hairlineGrid,
  indexLabel,
  type Icon,
} from "@/components/features/cells";
import { FeatureChapter } from "@/components/features/feature-chapter";
import { plannedMilestones } from "@/components/features/roadmap-data";
import { CURRENT_VERSION } from "@/lib/site";
import { cx } from "@/utils/cx";

export const metadata: Metadata = {
  title: "Features",
  description: `What W0rkTree ${CURRENT_VERSION} does today: snapshots, nested trees, staged visibility, a multi-tenant server, branch protection, license rules and Git interop.`,
};

interface FeatureItem {
  title: string;
  description: string;
}

interface FeatureSection {
  id: string;
  /** The short label in the chapter bar. */
  nav: string;
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
    nav: "Staged",
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
    nav: "Trees",
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
    nav: "Branches",
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
    nav: "Access",
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
    nav: "Licensing",
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
    nav: "Daemon",
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
    nav: "Sync",
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
    nav: "Config",
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
    nav: "Safety",
    badge: "Non-Destructive by Design",
    title: "Every change can be undone. Nothing is lost.",
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
      {
        title: "Undo & Redo",
        description:
          "Every local ref change is logged; wt undo and wt redo walk the log.",
      },
      {
        title: "Guarded Force Push",
        description:
          "Rewinding a server branch needs branch:force_push and a lease, and protections can forbid it.",
      },
    ],
    command: "wt revert HEAD",
    learnMore: { label: "Recovery guide", href: "/guides/reflog-and-recovery" },
  },
  {
    id: "security",
    nav: "Security",
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
    nav: "Git",
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
    nav: "Admin",
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

const chapters = [
  ...sections.map((section) => ({ id: section.id, label: section.nav })),
  { id: "roadmap", label: "Roadmap" },
];

export default function FeaturesPage() {
  return (
    <>
      <PageHero
        eyebrow={`Features · ${CURRENT_VERSION}`}
        title="Version control,"
        muted="rebuilt from the protocol up."
        actions={
          <>
            <ActionLink href="/guides/quick-start" trailingIcon={RiArrowRightLine}>
              Get Started
            </ActionLink>
            <ActionLink href="/docs/protocol" variant="secondary" leadingIcon={RiBookOpenLine}>
              Read the Protocol
            </ActionLink>
          </>
        }
        meta={`${sections.length} feature areas, from the object model to the admin panel`}
      >
        Not a Git wrapper or extension: a complete system with its own object model, sync protocol
        and server, plus Git import and export as a migration bridge.
      </PageHero>

      {/* The chapter bar stays pinned only while the chapters are on screen. */}
      <div>
        <ChapterNav chapters={chapters} label="Feature areas" />
        {sections.map((section, index) => (
          <FeatureChapter
            key={section.id}
            id={section.id}
            index={indexLabel(index)}
            eyebrow={section.badge}
            title={section.title}
            intro={section.intro}
            points={section.items.map((item) => ({
              lead: `${item.title}.`,
              body: item.description,
            }))}
            command={section.command}
            learnMore={section.learnMore}
            reverse={index % 2 === 1}
            sunken={index % 2 === 1}
          />
        ))}
        <RoadmapSection index={indexLabel(sections.length)} />
      </div>

      <FinalCta />
    </>
  );
}

/** What is planned but not shipped, read from the roadmap's own milestones so the two pages agree. */
function RoadmapSection({ index }: { index: string }) {
  return (
    <Band id="roadmap">
      <div className={cx("grid gap-8 py-16 md:py-20 lg:grid-cols-12", gutter)}>
        <SectionHeading
          index={index}
          eyebrow="Roadmap"
          title={`Not in ${CURRENT_VERSION} yet`}
          className="lg:col-span-7"
        >
          The work below is planned but not shipped yet. It is tracked on the roadmap, where
          priorities can change.
        </SectionHeading>
        <div className="flex items-end lg:col-span-4 lg:col-start-9 lg:justify-end">
          <TextLink href="/roadmap" trailingIcon={RiArrowRightLine}>
            View the roadmap
          </TextLink>
        </div>
      </div>
      <StaggerList
        className={cx(
          hairlineGrid,
          "border-t border-separator-border sm:grid-cols-2 lg:grid-cols-4",
        )}
      >
        {plannedMilestones.map((milestone) => (
          <li key={milestone.title} className="bg-background-full">
            <StaggerChild className="h-full">
              <Link href="/roadmap#planned" className={cellLink}>
                <span aria-hidden className="hatch pointer-events-none absolute inset-0 opacity-50" />
                <span className="relative flex items-center gap-2">
                  <span className="eyebrow">Planned</span>
                  <CellArrow className="ms-auto" />
                </span>
                <span className="relative text-title-3-semibold">{milestone.title}</span>
                <ul className="relative flex flex-col gap-2">
                  {milestone.items.map((item) => (
                    <RuleItem key={item} className="text-body-2-regular leading-5">
                      {item}
                    </RuleItem>
                  ))}
                </ul>
              </Link>
            </StaggerChild>
          </li>
        ))}
      </StaggerList>
    </Band>
  );
}

function FinalCta() {
  const paths: { icon: Icon; title: string; href: string }[] = [
    { icon: RiArrowRightLine, title: "Get Started", href: "/guides/quick-start" },
    { icon: RiTerminalBoxLine, title: "CLI Reference", href: "/docs/cli" },
    { icon: RiCodeSSlashLine, title: "Read the Protocol", href: "/docs/protocol" },
    { icon: RiHistoryLine, title: "Changelog", href: "/changelog" },
  ];
  return (
    <Band marks={false}>
      <div className="grid lg:grid-cols-2">
        <div
          className={cx(
            "relative flex flex-col items-start gap-6 overflow-hidden border-b border-separator-border py-16 md:py-20 lg:border-e lg:border-b-0",
            gutter,
          )}
        >
          <div
            aria-hidden
            className="hatch hatch-drift pointer-events-none absolute inset-0 opacity-60"
          />
          <Reveal className="relative flex flex-col items-start gap-6">
            <p className="eyebrow">{CURRENT_VERSION}</p>
            <h2 className="max-w-md text-display-4-semibold tracking-[-0.03em] text-balance md:text-display-3-semibold md:leading-[1.12]">
              Ready to evolve your workflow?
            </h2>
            <div className="mt-2 flex flex-wrap gap-2.5">
              <ActionLink href="/guides/quick-start" trailingIcon={RiArrowRightLine}>
                Get Started
              </ActionLink>
              <ActionLink href="/docs/cli" variant="secondary" leadingIcon={RiTerminalBoxLine}>
                CLI Reference
              </ActionLink>
            </div>
          </Reveal>
        </div>
        <StaggerList className={cx(hairlineGrid, "sm:grid-cols-2")}>
          {paths.map(({ icon: PathIcon, ...path }) => (
            <li key={path.href} className="bg-background-full">
              <StaggerChild className="h-full">
                <Link href={path.href} className={cx(cellLink, "gap-2")}>
                  <HoverHatch />
                  <IconTile icon={PathIcon} className="relative" />
                  <span className="relative mt-2 flex items-center gap-1.5 text-headline-medium">
                    {path.title}
                    <CellArrow />
                  </span>
                  <span className="relative font-mono text-caption-1-regular text-text-secondary">
                    {path.href}
                  </span>
                </Link>
              </StaggerChild>
            </li>
          ))}
        </StaggerList>
      </div>
      <p
        className={cx(
          "flex flex-wrap items-center gap-x-2 gap-y-1 border-t border-separator-border py-5 text-body-regular text-text-secondary",
          gutter,
        )}
      >
        <RiRoadMapLine className="size-4 shrink-0 text-foreground-icon-tertiary" aria-hidden />
        <span>Planned work is on the</span>
        <TextLink href="/roadmap" trailingIcon={RiArrowRightLine} className="inline-flex">
          roadmap
        </TextLink>
      </p>
    </Band>
  );
}
