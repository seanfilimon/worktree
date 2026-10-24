"use client";

import * as React from "react";
import Link from "next/link";
import {
  FolderTree,
  Database,
  Eye,
  Clock,
  RefreshCw,
  User,
  FileText,
  ShieldCheck,
  ScanSearch,
  ArrowRightLeft,
  type LucideIcon,
} from "lucide-react";
import { motion, AnimatePresence } from "motion/react";
import {
  IoEye,
  IoPeople,
  IoStatsChart,
  IoArrowForward,
  IoShieldCheckmark,
  IoGitNetwork,
  IoGitBranch,
  IoFlash,
  IoLogoGithub,
  IoLogoDiscord,
  IoTerminal,
  IoCopy,
  IoServer,
  IoHelpCircle,
  IoSwapHorizontal,
  IoExtensionPuzzle,
  IoSchool,
  IoDocumentText,
  IoHeart,
  IoFingerPrint,
  IoLayers,
  IoSpeedometer,
  IoCloudUpload,
  IoGrid,
} from "react-icons/io5";
import type { IconType } from "react-icons";
import { SiteHeader } from "@/components/site-header";
import { CopyCommand } from "@/components/copy-command";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  CURRENT_VERSION,
  DEFAULT_SERVER_URL,
  DISCORD_URL,
  LICENSE_NAME,
  REPO_URL,
} from "@/lib/site";

// ─── Content ─────────────────────────────────────────────────────────────────

const installTabs = [
  {
    value: "cli",
    label: "CLI",
    command: `git clone ${REPO_URL}.git && cd worktree\ncargo install --path crates/worktree-cli\ncargo install --path crates/worktree-bg`,
  },
  {
    value: "server",
    label: "Server",
    command:
      "cd services/server-go\ngo build -o worktree-server .\n./worktree-server --data ./wt-data --admin-password change-me",
  },
  {
    value: "admin",
    label: "Admin panel",
    command:
      "cd apps/admin && npm install && npm run build\n./worktree-server --admin-dir apps/admin/dist",
  },
  {
    value: "first-push",
    label: "First push",
    command: `wt init && wt snapshot -m "first snapshot"\nwt register ${DEFAULT_SERVER_URL} -u alice -e alice@example.com\nwt remote add origin ${DEFAULT_SERVER_URL} --tenant alice\nwt push`,
  },
] as const;

interface FeatureCard {
  title: string;
  badge: string;
  cmd: string;
  desc: string;
  href: string;
  Icon: LucideIcon;
}

const featureCards: FeatureCard[] = [
  {
    title: "Nested Trees",
    badge: "Core",
    cmd: "wt tree add frontend frontend",
    desc: "Independent branches, history and HEAD per tree inside one worktree.",
    href: "/guides/nested-trees",
    Icon: FolderTree,
  },
  {
    title: "Snapshots & History",
    badge: "Core",
    cmd: 'wt snapshot -m "feat: add login"',
    desc: "BLAKE3 content-addressed objects and an append-only snapshot graph.",
    href: "/docs/cli/snapshots",
    Icon: Database,
  },
  {
    title: "Staged Visibility",
    badge: "Collaboration",
    cmd: "wt status --team",
    desc: "See your team's work in progress before anyone pushes.",
    href: "/guides/staged-visibility",
    Icon: Eye,
  },
  {
    title: "Auto Tracking",
    badge: "Automation",
    cmd: "wt daemon start",
    desc: "A background daemon takes auto-snapshots and stages your WIP.",
    href: "/guides/auto-tracking-setup",
    Icon: Clock,
  },
  {
    title: "Sync Protocol",
    badge: "Infrastructure",
    cmd: "wt push",
    desc: "gRPC over one HTTP/2 port; only missing objects are transferred.",
    href: "/docs/protocol/sync-protocol",
    Icon: RefreshCw,
  },
  {
    title: "Multi-Tenant IAM",
    badge: "Security",
    cmd: "wt clone http://localhost:7420 acme/app",
    desc: "Personal and org tenants, five built-in roles, per-tree visibility.",
    href: "/guides/multi-tenancy",
    Icon: User,
  },
  {
    title: "License Compliance",
    badge: "Governance",
    cmd: "PUT /api/v1/trees/{tenant}/{tree}/licenses",
    desc: "Per-path SPDX rules checked by the server on every push.",
    href: "/guides/license-compliance",
    Icon: FileText,
  },
  {
    title: "Branch Protection",
    badge: "Governance",
    cmd: "PUT /api/v1/trees/{tenant}/{tree}/protections",
    desc: "Block direct pushes, require approvals, merge through merge requests.",
    href: "/guides/branch-protection",
    Icon: ShieldCheck,
  },
  {
    title: "Secret Scanning",
    badge: "Security",
    cmd: "wt snapshot  # scans before writing",
    desc: "Credentials and private keys are caught before they enter history.",
    href: "/docs/security",
    Icon: ScanSearch,
  },
  {
    title: "Git Import & Export",
    badge: "Migration",
    cmd: "wt git import ./repo ./repo-wt",
    desc: "Bring Git history in, or export snapshots back to a Git repository.",
    href: "/guides/migration",
    Icon: ArrowRightLeft,
  },
];

interface Comparison {
  cat: string;
  icon: IconType | LucideIcon;
  color: string;
  bg: string;
  git: string;
  wt: string;
}

const comparisons: Comparison[] = [
  {
    cat: "UX & Workflow",
    icon: IoTerminal,
    color: "text-blue-500",
    bg: "bg-blue-500/10",
    git: "Git has 150+ commands, many overloaded (checkout switches branches and restores files). The index is an extra step to learn.",
    wt: "No staging area: a snapshot captures the working tree. switch, restore and revert each do exactly one thing.",
  },
  {
    cat: "Safety & Recovery",
    icon: IoShieldCheckmark,
    color: "text-emerald-500",
    bg: "bg-emerald-500/10",
    git: "Commands like `git reset --hard` and `git push --force` destroy work and rewrite shared history.",
    wt: "History is append-only. Reverts add snapshots, branches are archived rather than deleted, and the server has no force push.",
  },
  {
    cat: "Collaboration",
    icon: IoPeople,
    color: "text-purple-500",
    bg: "bg-purple-500/10",
    git: "Work stays invisible until it is pushed, so overlapping changes surface late as merge conflicts.",
    wt: "Staged snapshots share in-flight work with the team. `wt status --team` shows who is changing what, on which branch.",
  },
  {
    cat: "Identity & Access",
    icon: User,
    color: "text-amber-500",
    bg: "bg-amber-500/10",
    git: "Git itself has no authentication or authorization; access control comes entirely from a hosting platform.",
    wt: "The server enforces tenants, five built-in roles and private/internal/public tree visibility on every gRPC and REST call.",
  },
  {
    cat: "Security",
    icon: IoServer,
    color: "text-rose-500",
    bg: "bg-rose-500/10",
    git: "Leaked credentials in a commit live in history forever unless the history is rewritten.",
    wt: "Snapshots are scanned for secrets before they are written. Passwords use argon2id and API tokens are stored only as hashes.",
  },
  {
    cat: "Observability",
    icon: IoStatsChart,
    color: "text-cyan-500",
    bg: "bg-cyan-500/10",
    git: "Git has no server of its own: dashboards, audit trails and activity feeds are left to third parties.",
    wt: "The server ships an append-only audit log, a live event stream, Prometheus metrics and a React admin panel.",
  },
  {
    cat: "License Compliance",
    icon: IoDocumentText,
    color: "text-yellow-500",
    bg: "bg-yellow-500/10",
    git: "Open-source and proprietary code mix freely. Compliance is an honor system.",
    wt: "Per-path SPDX rules with read-only, modify or redistribute grants. The server rejects pushes that violate them.",
  },
  {
    cat: "Multi-Project Org",
    icon: FolderTree,
    color: "text-indigo-500",
    bg: "bg-indigo-500/10",
    git: "Submodules are brittle and detached. Monorepos need sparse-checkout rules to stay manageable.",
    wt: "Nested trees live in one worktree, each with its own branches, HEAD and history.",
  },
];

interface InternalsCard {
  label: string;
  description: string;
  detail: string;
  badge: string;
  icon: IconType;
}

const internals: InternalsCard[] = [
  {
    label: "BLAKE3 hashing",
    description: "Content addressing",
    detail: "Every blob and manifest is named by the BLAKE3-256 hash of its exact bytes.",
    badge: "Integrity",
    icon: IoFingerPrint,
  },
  {
    label: "Deduplication",
    description: "Object negotiation",
    detail: "Identical content is stored once; push and pull transfer only the objects the other side lacks.",
    badge: "Efficient",
    icon: IoLayers,
  },
  {
    label: "Stat cache",
    description: "Fast status",
    detail: "A per-tree index of size and mtime skips rehashing files that have not changed.",
    badge: "Local",
    icon: IoSpeedometer,
  },
  {
    label: "Streaming sync",
    description: "gRPC over h2c",
    detail: "Objects stream in both directions with a 64 MiB message limit, on the same port as REST.",
    badge: "One port",
    icon: IoCloudUpload,
  },
  {
    label: "Single binary",
    description: "Pure Go server",
    detail: "Metadata in an embedded bbolt database and objects on disk. No external database to run.",
    badge: "Self-host",
    icon: IoGrid,
  },
];

interface Faq {
  icon: IconType;
  q: string;
  a: string;
}

const faqs: Faq[] = [
  {
    icon: IoGitNetwork,
    q: "Is W0rkTree built on Git?",
    a: "No. W0rkTree has its own object model (BLAKE3-addressed blobs and manifests), its own snapshot history and its own sync protocol (gRPC, defined in worktree.sync.v1). It is not a Git wrapper. Git interoperability is provided by explicit `wt git import` and `wt git export` commands.",
  },
  {
    icon: IoShieldCheckmark,
    q: "What is a nested tree?",
    a: "A tree is a directory with its own branches, HEAD, reflog and snapshot history. A worktree always has a root tree; `wt tree add` registers sub-directories as nested trees, and each tree's snapshots exclude the paths of trees nested inside it.",
  },
  {
    icon: IoSwapHorizontal,
    q: "Can I migrate from Git?",
    a: "Yes. `wt git import` converts a Git repository's history into W0rkTree snapshots, and `wt git export` writes snapshots back to a Git repository, so you can keep Git-only consumers working during a transition.",
  },
  {
    icon: IoExtensionPuzzle,
    q: "How do I use W0rkTree in CI?",
    a: "Clone with `wt clone`, authenticate with a personal API token, and use `--json` for machine-readable output. First-party CI integrations and required status checks are on the roadmap.",
  },
  {
    icon: IoSchool,
    q: "Is it ready for production?",
    a: `Not yet. ${CURRENT_VERSION} is a pre-1.0 rebuild that you build from source. The CLI, background daemon, server and admin panel work end to end, but on-disk and wire formats may still change before 1.0.`,
  },
  {
    icon: IoEye,
    q: "What are staged snapshots?",
    a: "A staged snapshot is your current work in progress, uploaded to the server so teammates can see it with `wt status --team`. It is not part of branch history, is replaced by your next stage and is cleared when you push.",
  },
  {
    icon: IoPeople,
    q: "How does multi-tenancy work?",
    a: "Every account gets a personal tenant named after its username, and organizations are separate tenants with members. Members hold one of five roles (owner, admin, maintainer, developer, viewer), and each tree is private, internal or public.",
  },
  {
    icon: IoDocumentText,
    q: "Does W0rkTree handle licensing?",
    a: "Yes. Tree admins attach SPDX license rules to path globs with a grant of read-only, modify or redistribute. The server checks pushes against those rules and rejects violations.",
  },
];

const stats = [
  {
    stat: "29",
    label: "CLI Commands",
    desc: "One verb per operation, each with --json output for scripting.",
  },
  {
    stat: "5",
    label: "Built-in Roles",
    desc: "Owner, admin, maintainer, developer and viewer, enforced by the server.",
  },
  {
    stat: "1",
    label: "Server Port",
    desc: "gRPC sync, REST API, live events and the admin panel all share :7420.",
  },
];

const footerColumns = [
  {
    heading: "Product",
    links: [
      { label: "Features", href: "/features" },
      { label: "Changelog", href: "/changelog" },
      { label: "Roadmap", href: "/roadmap" },
      { label: "Security", href: "/security" },
      { label: "Staged Visibility", href: "/features#staged-visibility" },
      { label: "Multi-Tenancy", href: "/features#iam" },
    ],
  },
  {
    heading: "Documentation",
    links: [
      { label: "Quick Start", href: "/guides/quick-start" },
      { label: "Installation", href: "/docs/installation" },
      { label: "CLI Reference", href: "/docs/cli" },
      { label: "REST API", href: "/docs/server/rest-api" },
      { label: "Sync Protocol", href: "/docs/protocol/sync-protocol" },
      { label: "Migration Guide", href: "/guides/migration" },
    ],
  },
  {
    heading: "Community",
    links: [
      { label: "GitHub", href: REPO_URL },
      { label: "Discord", href: DISCORD_URL },
      { label: "Articles", href: "/articles" },
      { label: "Contributing", href: "/contributing" },
      { label: "Code of Conduct", href: "/code-of-conduct" },
    ],
  },
];

/** Renders `backtick` spans in copy text as inline code. */
function withInlineCode(text: string): React.ReactNode[] {
  return text.split("`").map((part, index) =>
    index % 2 === 1 ? (
      <code key={index} className="rounded bg-muted px-1 py-0.5 font-mono text-[0.85em]">
        {part}
      </code>
    ) : (
      part
    ),
  );
}

// ─── Page ────────────────────────────────────────────────────────────────────

type ComparisonView = "git" | "wt";

export default function HomePage() {
  const [view, setView] = React.useState<ComparisonView>("wt");

  return (
    <div className="flex min-h-screen flex-col bg-background">
      <SiteHeader />

      <main className="flex-1">
        {/* Hero */}
        <section className="relative overflow-hidden border-b">
          <div className="absolute inset-0 bg-gradient-to-b from-muted/50 to-background" />

          <div className="relative container mx-auto px-4 py-24 md:py-32">
            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.5, ease: "easeOut" }}
              className="mx-auto max-w-4xl text-center"
            >
              <motion.div
                initial={{ opacity: 0, scale: 0.9 }}
                animate={{ opacity: 1, scale: 1 }}
                transition={{ duration: 0.3, delay: 0.1 }}
              >
                <Badge variant="outline" className="mb-6">
                  <span className="relative mr-2 flex h-2 w-2" aria-hidden="true">
                    <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-primary opacity-75" />
                    <span className="relative inline-flex h-2 w-2 rounded-full bg-primary" />
                  </span>
                  {CURRENT_VERSION} · The v2 rebuild
                </Badge>
              </motion.div>

              <motion.h1
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.5, delay: 0.2 }}
                className="mb-6 text-4xl font-bold tracking-tight sm:text-6xl md:text-7xl"
              >
                Multi-Tenant
                <br />
                <span className="text-primary">Version Control.</span>
              </motion.h1>

              <motion.p
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.5, delay: 0.3 }}
                className="mx-auto mb-8 max-w-2xl text-lg text-muted-foreground md:text-xl"
              >
                Snapshots, nested trees and team-visible work in progress, backed
                by a self-hosted multi-tenant server. A Rust CLI and daemon, a Go
                server, and Git import and export.
              </motion.p>

              <motion.div
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.5, delay: 0.4 }}
                className="mb-12 flex flex-col items-center justify-center gap-4 sm:flex-row"
              >
                <Button size="lg" className="group gap-2" asChild>
                  <Link href="/guides/quick-start">
                    Get Started
                    <IoArrowForward
                      className="h-4 w-4 transition-transform group-hover:translate-x-1"
                      aria-hidden="true"
                    />
                  </Link>
                </Button>
                <Button size="lg" variant="outline" className="gap-2" asChild>
                  <Link href="/docs/protocol">
                    <IoGitBranch className="h-4 w-4" aria-hidden="true" />
                    View Protocol
                  </Link>
                </Button>
              </motion.div>

              {/* Build from source */}
              <motion.div
                initial={{ opacity: 0, y: 30 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.6, delay: 0.5 }}
                className="mx-auto max-w-2xl"
              >
                <Card>
                  <CardHeader className="pb-3">
                    <CardTitle className="flex items-center gap-2 text-lg">
                      <IoTerminal className="h-5 w-5" aria-hidden="true" />
                      Build from source
                    </CardTitle>
                  </CardHeader>
                  <CardContent>
                    <Tabs defaultValue="cli" className="w-full">
                      <TabsList className="grid w-full grid-cols-2 sm:grid-cols-4">
                        {installTabs.map((tab) => (
                          <TabsTrigger key={tab.value} value={tab.value}>
                            {tab.label}
                          </TabsTrigger>
                        ))}
                      </TabsList>
                      {installTabs.map((tab) => (
                        <TabsContent key={tab.value} value={tab.value} className="mt-4">
                          <CopyCommand command={tab.command} />
                        </TabsContent>
                      ))}
                    </Tabs>
                    <p className="mt-3 text-left text-xs text-muted-foreground">
                      Requires Rust (stable) and Go 1.25+; Node 22+ for the admin
                      panel. See the{" "}
                      <Link href="/docs/installation" className="underline underline-offset-2 hover:text-foreground">
                        installation guide
                      </Link>
                      .
                    </p>
                  </CardContent>
                </Card>
              </motion.div>
            </motion.div>
          </div>
        </section>

        {/* Features */}
        <section className="border-b py-16 md:py-24" aria-labelledby="features-heading">
          <div className="w-full">
            <div className="mx-auto mb-16 max-w-3xl px-4 text-center">
              <Badge variant="secondary" className="mb-4">
                Features
              </Badge>
              <h2 id="features-heading" className="mb-4 text-3xl font-bold">
                Protocol Architecture
              </h2>
              <p className="text-lg text-muted-foreground">
                Multi-tenant version control with built-in coordination
              </p>
            </div>

            <motion.div
              initial={{ opacity: 0, y: 30 }}
              whileInView={{ opacity: 1, y: 0 }}
              viewport={{ once: true }}
              transition={{ duration: 0.5 }}
              className="grid w-full grid-cols-1 sm:grid-cols-2 lg:grid-cols-5"
            >
              {featureCards.map((card) => (
                <Link
                  key={card.title}
                  href={card.href}
                  className="-ml-px -mt-px flex flex-col items-start gap-4 border border-border bg-card p-5 transition-colors hover:bg-accent/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                >
                  <div className="flex w-full items-start justify-between">
                    <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-primary/10 text-primary">
                      <card.Icon className="h-5 w-5" aria-hidden="true" />
                    </div>
                    <Badge variant="outline" className="text-[10px] uppercase tracking-wider">
                      {card.badge}
                    </Badge>
                  </div>
                  <div>
                    <h3 className="mb-1 text-base font-semibold">{card.title}</h3>
                    <p className="text-sm text-muted-foreground">{card.desc}</p>
                  </div>
                  <code className="mt-auto w-full truncate rounded bg-muted px-2 py-1.5 font-mono text-[10px] text-muted-foreground">
                    {card.cmd}
                  </code>
                </Link>
              ))}
            </motion.div>
          </div>
        </section>

        {/* Why W0rkTree */}
        <section className="border-b bg-muted/30 py-16 md:py-24" aria-labelledby="why-heading">
          <div className="w-full">
            <div className="mx-auto mb-12 max-w-3xl px-4 text-center">
              <Badge variant="secondary" className="mb-4">
                Why W0rkTree
              </Badge>
              <h2 id="why-heading" className="mb-4 text-3xl font-bold">
                Stop fighting your tools
              </h2>
              <p className="mx-auto max-w-xl text-lg text-muted-foreground">
                A version control system built for multi-team collaboration.
              </p>
            </div>

            <div className="w-full">
              <div className="mb-8 flex justify-center px-4">
                <div
                  className="relative flex w-full max-w-[400px] rounded-lg bg-muted/50 p-1"
                  role="group"
                  aria-label="Compare Git and W0rkTree"
                >
                  <button
                    type="button"
                    onClick={() => setView("git")}
                    aria-pressed={view === "git"}
                    className={`relative z-10 flex w-full items-center justify-center py-2 text-[11px] font-semibold uppercase tracking-wider transition-colors ${
                      view === "git"
                        ? "text-destructive"
                        : "text-muted-foreground hover:text-foreground"
                    }`}
                  >
                    <IoCopy className="mr-1.5 h-3.5 w-3.5" aria-hidden="true" />
                    Git Problem
                  </button>
                  <button
                    type="button"
                    onClick={() => setView("wt")}
                    aria-pressed={view === "wt"}
                    className={`relative z-10 flex w-full items-center justify-center py-2 text-[11px] font-semibold uppercase tracking-wider transition-colors ${
                      view === "wt"
                        ? "text-primary"
                        : "text-muted-foreground hover:text-foreground"
                    }`}
                  >
                    <IoFlash className="mr-1.5 h-3.5 w-3.5" aria-hidden="true" />
                    W0rkTree Answer
                  </button>
                  <motion.div
                    aria-hidden="true"
                    className={`absolute inset-y-1 w-[calc(50%-4px)] rounded-md shadow-sm ${
                      view === "git" ? "bg-destructive/10" : "bg-primary/10"
                    }`}
                    initial={false}
                    animate={{ left: view === "git" ? "4px" : "calc(50%)" }}
                    transition={{ type: "spring", bounce: 0.2, duration: 0.5 }}
                  />
                </div>
              </div>

              <motion.div
                initial={{ opacity: 0, y: 20 }}
                whileInView={{ opacity: 1, y: 0 }}
                viewport={{ once: true }}
                transition={{ duration: 0.4 }}
                className="grid w-full grid-cols-1 md:grid-cols-2 xl:grid-cols-4"
                aria-live="polite"
              >
                {comparisons.map((row) => (
                  <div
                    key={row.cat}
                    className="-ml-px -mt-px flex flex-col gap-4 border border-border bg-card p-6"
                  >
                    <div className="flex items-center gap-3">
                      <div
                        className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-lg ${row.bg} ${row.color}`}
                      >
                        <row.icon className="h-5 w-5" aria-hidden="true" />
                      </div>
                      <h3 className="text-base font-bold text-foreground">{row.cat}</h3>
                    </div>
                    <div className="mt-auto min-h-[110px]">
                      <AnimatePresence mode="wait">
                        {view === "git" ? (
                          <motion.p
                            key="git"
                            initial={{ opacity: 0, filter: "blur(4px)" }}
                            animate={{ opacity: 1, filter: "blur(0px)" }}
                            exit={{ opacity: 0, filter: "blur(4px)" }}
                            transition={{ duration: 0.2 }}
                            className="h-full rounded-lg border border-destructive/10 bg-destructive/5 p-4 text-sm leading-relaxed text-muted-foreground"
                          >
                            {withInlineCode(row.git)}
                          </motion.p>
                        ) : (
                          <motion.p
                            key="wt"
                            initial={{ opacity: 0, filter: "blur(4px)" }}
                            animate={{ opacity: 1, filter: "blur(0px)" }}
                            exit={{ opacity: 0, filter: "blur(4px)" }}
                            transition={{ duration: 0.2 }}
                            className="h-full rounded-lg border border-primary/10 bg-primary/5 p-4 text-sm leading-relaxed text-foreground"
                          >
                            {withInlineCode(row.wt)}
                          </motion.p>
                        )}
                      </AnimatePresence>
                    </div>
                  </div>
                ))}
              </motion.div>
            </div>
          </div>
        </section>

        {/* Under the hood */}
        <section className="border-b bg-muted/10 py-16 md:py-24" aria-labelledby="internals-heading">
          <div className="w-full">
            <div className="mx-auto mb-12 max-w-3xl px-4 text-center">
              <Badge variant="secondary" className="mb-4">
                Under the Hood
              </Badge>
              <h2 id="internals-heading" className="mb-4 text-3xl font-bold">
                Built for Integrity & Scale
              </h2>
              <p className="text-lg text-muted-foreground">
                The design choices that keep history verifiable and sync cheap.
              </p>
            </div>

            <motion.div
              initial={{ opacity: 0, y: 20 }}
              whileInView={{ opacity: 1, y: 0 }}
              viewport={{ once: true }}
              transition={{ duration: 0.4 }}
              className="grid w-full grid-cols-1 sm:grid-cols-2 lg:grid-cols-5"
            >
              {internals.map((item) => (
                <div
                  key={item.label}
                  className="-ml-px -mt-px flex flex-col items-start gap-4 border border-border bg-card p-6"
                >
                  <div className="flex w-full items-start justify-between">
                    <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-primary/10 text-primary">
                      <item.icon className="h-5 w-5" aria-hidden="true" />
                    </div>
                    <Badge
                      variant="outline"
                      className="border-emerald-500/30 bg-emerald-500/10 text-[10px] uppercase tracking-wider text-emerald-600 dark:text-emerald-400"
                    >
                      {item.badge}
                    </Badge>
                  </div>
                  <div>
                    <h3 className="mb-1 text-base font-semibold">{item.label}</h3>
                    <p className="text-sm text-muted-foreground">{item.description}</p>
                  </div>
                  <p className="mt-auto w-full border-t border-border pt-4 text-sm text-foreground/90">
                    {item.detail}
                  </p>
                </div>
              ))}
            </motion.div>
          </div>
        </section>

        {/* FAQ */}
        <section className="border-b bg-muted/30 py-16 md:py-24" aria-labelledby="faq-heading">
          <div className="container mx-auto px-4">
            <div className="mx-auto max-w-5xl">
              <div className="mb-14 text-center">
                <Badge variant="secondary" className="mb-4">
                  <IoHelpCircle className="mr-1 h-3 w-3" aria-hidden="true" />
                  FAQ
                </Badge>
                <h2 id="faq-heading" className="mb-4 text-3xl font-bold">
                  Common Questions
                </h2>
                <p className="mx-auto max-w-2xl text-lg text-muted-foreground">
                  Everything you need to know before getting started
                </p>
              </div>

              <motion.div
                initial={{ opacity: 0, y: 20 }}
                whileInView={{ opacity: 1, y: 0 }}
                viewport={{ once: true }}
                transition={{ duration: 0.4 }}
                className="overflow-hidden rounded-xl border border-border"
              >
                {faqs.map((faq, i) => (
                  <details
                    key={faq.q}
                    className={`group bg-card ${i > 0 ? "border-t border-border" : ""}`}
                  >
                    <summary className="flex cursor-pointer list-none select-none items-center gap-4 p-5 transition-colors hover:bg-accent/30 md:p-6 [&::-webkit-details-marker]:hidden">
                      <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 ring-1 ring-primary/20">
                        <faq.icon className="h-4 w-4 text-primary" aria-hidden="true" />
                      </div>
                      <span className="flex-1 text-sm font-semibold">{faq.q}</span>
                      <IoArrowForward
                        className="h-4 w-4 shrink-0 text-muted-foreground transition-transform duration-200 group-open:rotate-90"
                        aria-hidden="true"
                      />
                    </summary>
                    <div className="px-5 pb-5 pl-[4.25rem] md:px-6 md:pb-6 md:pl-[4.75rem]">
                      <p className="text-sm leading-relaxed text-muted-foreground">{withInlineCode(faq.a)}</p>
                    </div>
                  </details>
                ))}
              </motion.div>
            </div>
          </div>
        </section>

        {/* By the numbers */}
        <section className="border-b py-16 md:py-24" aria-labelledby="numbers-heading">
          <div className="container mx-auto px-4">
            <div className="mx-auto max-w-5xl">
              <div className="mb-14 text-center">
                <Badge variant="secondary" className="mb-4">
                  <IoStatsChart className="mr-1 h-3 w-3" aria-hidden="true" />
                  By the Numbers
                </Badge>
                <h2 id="numbers-heading" className="mb-4 text-3xl font-bold">
                  Small Surface, Clear Contracts
                </h2>
                <p className="mx-auto max-w-2xl text-lg text-muted-foreground">
                  A focused CLI and one server process, specified by a single
                  protobuf contract.
                </p>
              </div>

              <div className="grid gap-px overflow-hidden rounded-xl border border-border bg-border md:grid-cols-3">
                {stats.map((s, i) => (
                  <motion.div
                    key={s.label}
                    initial={{ opacity: 0, y: 20 }}
                    whileInView={{ opacity: 1, y: 0 }}
                    viewport={{ once: true }}
                    transition={{ duration: 0.4, delay: i * 0.1 }}
                    className="flex flex-col items-center bg-card p-8 text-center"
                  >
                    <div className="mb-2 text-5xl font-bold text-primary">{s.stat}</div>
                    <div className="mb-3 text-lg font-semibold">{s.label}</div>
                    <p className="text-sm text-muted-foreground">{s.desc}</p>
                  </motion.div>
                ))}
              </div>
            </div>
          </div>
        </section>

        {/* CTA */}
        <section className="relative overflow-hidden py-20 md:py-32" aria-labelledby="cta-heading">
          <div className="absolute inset-0 bg-primary/[0.03]" />
          <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_center,hsl(var(--primary)/0.08),transparent_70%)]" />
          <div className="container relative mx-auto px-4">
            <div className="mx-auto max-w-2xl text-center">
              <motion.div
                initial={{ opacity: 0, y: 20 }}
                whileInView={{ opacity: 1, y: 0 }}
                viewport={{ once: true }}
                transition={{ duration: 0.5 }}
                className="space-y-6"
              >
                <div className="inline-flex items-center gap-2 rounded-full border border-primary/20 bg-primary/5 px-4 py-1.5 text-xs font-medium text-primary">
                  <IoFlash className="h-3 w-3" aria-hidden="true" />
                  Open source · {LICENSE_NAME}
                </div>
                <h2 id="cta-heading" className="text-4xl font-bold tracking-tight md:text-5xl">
                  Ready to try it?
                </h2>
                <p className="mx-auto max-w-lg text-lg text-muted-foreground">
                  Build the CLI and server from source, start a local server, and
                  push your first snapshot in a few minutes.
                </p>

                <div className="flex flex-col justify-center gap-3 pt-2 sm:flex-row">
                  <Button size="lg" className="group h-12 gap-2 px-8" asChild>
                    <Link href="/guides/quick-start">
                      Get Started
                      <IoArrowForward
                        className="h-4 w-4 transition-transform group-hover:translate-x-1"
                        aria-hidden="true"
                      />
                    </Link>
                  </Button>
                  <Button size="lg" variant="outline" className="h-12 gap-2 px-8" asChild>
                    <a href={REPO_URL} target="_blank" rel="noopener noreferrer">
                      <IoLogoGithub className="h-4 w-4" aria-hidden="true" />
                      Star on GitHub
                    </a>
                  </Button>
                </div>

                <div className="mx-auto max-w-md pt-4">
                  <CopyCommand
                    command="cargo install --path crates/worktree-cli"
                    prompt
                    className="border border-border bg-muted/60"
                  />
                </div>
              </motion.div>
            </div>
          </div>
        </section>
      </main>

      <footer className="border-t bg-card">
        <div className="container mx-auto px-4 py-14">
          <div className="grid gap-10 md:grid-cols-2 lg:grid-cols-[1.5fr_1fr_1fr_1fr]">
            <div className="space-y-4">
              <div className="flex items-center gap-2.5">
                <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-primary/10">
                  <IoGitNetwork className="h-4 w-4 text-primary" aria-hidden="true" />
                </div>
                <span className="text-lg font-bold">W0rkTree</span>
              </div>
              <p className="max-w-xs text-sm leading-relaxed text-muted-foreground">
                Version control with nested trees, automatic tracking and a
                multi-tenant server. Rust client, Go server.
              </p>
              <div className="flex items-center gap-3 pt-1">
                <a
                  href={REPO_URL}
                  target="_blank"
                  rel="noopener noreferrer"
                  aria-label="W0rkTree on GitHub"
                  className="flex h-8 w-8 items-center justify-center rounded-lg border border-border bg-background text-muted-foreground transition-colors hover:border-foreground/20 hover:text-foreground"
                >
                  <IoLogoGithub className="h-4 w-4" aria-hidden="true" />
                </a>
                <a
                  href={DISCORD_URL}
                  target="_blank"
                  rel="noopener noreferrer"
                  aria-label="W0rkTree on Discord"
                  className="flex h-8 w-8 items-center justify-center rounded-lg border border-border bg-background text-muted-foreground transition-colors hover:border-foreground/20 hover:text-foreground"
                >
                  <IoLogoDiscord className="h-4 w-4" aria-hidden="true" />
                </a>
              </div>
            </div>

            {footerColumns.map((column) => (
              <nav key={column.heading} aria-label={column.heading}>
                <h2 className="mb-4 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                  {column.heading}
                </h2>
                <ul className="space-y-2.5">
                  {column.links.map((link) => (
                    <li key={link.label}>
                      <Link
                        href={link.href}
                        className="text-sm text-muted-foreground transition-colors hover:text-foreground"
                      >
                        {link.label}
                      </Link>
                    </li>
                  ))}
                </ul>
              </nav>
            ))}
          </div>
        </div>

        <div className="border-t border-border">
          <div className="container mx-auto flex flex-col items-center justify-between gap-4 px-4 py-5 md:flex-row">
            <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <span>Built with</span>
              <IoHeart className="h-3 w-3 text-primary" aria-label="love" />
              <span>by the W0rkTree contributors</span>
              <span className="mx-1.5" aria-hidden="true">
                ·
              </span>
              <span>{LICENSE_NAME}</span>
            </div>
            <div className="flex items-center gap-4 text-xs text-muted-foreground">
              <Link href="/privacy" className="transition-colors hover:text-foreground">
                Privacy
              </Link>
              <Link href="/terms" className="transition-colors hover:text-foreground">
                Terms
              </Link>
              <span>© 2025–2026 W0rkTree</span>
            </div>
          </div>
        </div>
      </footer>
    </div>
  );
}
