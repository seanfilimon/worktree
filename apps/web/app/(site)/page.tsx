import type { ReactNode } from "react";
import Link from "next/link";
import { highlight } from "fumadocs-core/highlight";
import {
  RiArrowRightLine,
  RiArrowRightUpLine,
  RiBookOpenLine,
  RiFingerprintLine,
  RiFlashlightLine,
  RiGitBranchLine,
  RiLayoutGridLine,
  RiServerLine,
  RiStackLine,
} from "react-icons/ri";
import { ChapterNav } from "@/components/home/chapter-nav";
import { CodeTourView, type TourStep } from "@/components/home/code-tour-view";
import { HeroGrid } from "@/components/home/hero-grid";
import { HeroScenes, type Scene } from "@/components/home/hero-scenes";
import { InstallCommand } from "@/components/home/install-command";
import { Reveal } from "@/components/home/motion";
import { SplitExplainer, StackedExplainer } from "@/components/home/section";
import {
  CompareTable,
  FaqList,
  withCode,
  ServerStack,
  TeamStatus,
  TreeDiagram,
  type Comparison,
  type Question,
} from "@/components/home/visuals";
import { ActionLink, TextLink } from "@/components/site/action";
import { CreatorCredit } from "@/components/site/creator-credit";
import { Band, SectionHeading, gutter } from "@/components/site/frame";
import {
  StaggerChild,
  StaggerList,
  WordReveal,
} from "@/components/site/motion-text";
import { codeThemes } from "@/lib/code-themes";
import { version } from "@/lib/shared";
import { DEFAULT_SERVER_URL, REPO_URL } from "@/lib/site";
import { cx } from "@/utils/cx";

const chapters = [
  { id: "model", label: "Model" },
  { id: "workflow", label: "Workflow" },
  { id: "team", label: "Team" },
  { id: "server", label: "Server" },
  { id: "governance", label: "Governance" },
  { id: "internals", label: "Internals" },
  { id: "compare", label: "Compare" },
  { id: "faq", label: "FAQ" },
] as const;

const scenes: (Omit<Scene, "code"> & { source: string })[] = [
  {
    title: "Track",
    summary: "Snapshots of the whole working tree, taken by you or the daemon.",
    source: `wt init acme && cd acme
wt tree add api services/api      # a nested tree with its own history
wt snapshot -m "first snapshot"   # no staging area: the working tree is the snapshot

wt daemon start && wt daemon watch
# from now on, 30 s after you stop typing, each changed tree is
# auto-snapshotted on its current branch and staged for the team`,
  },
  {
    title: "Share",
    summary: "Work in progress the team can see, before anything is pushed.",
    source: `wt stage --label spike    # one of up to ten labeled slots per branch
wt status --team          # who has work in progress, on which branch and paths

wt push                   # only missing objects, packed with binary deltas;
                          # notes who else has staged the same paths`,
  },
  {
    title: "Govern",
    summary: "Protections, licenses and secret scanning on every push.",
    source: `wt protect set main --approvals 1 --no-direct-push
wt mr create --source feature/auth --target main --title "Add token auth"
wt mr merge 12 --rebase --delete-branch

wt compliance findings    # secrets the server found in pushes
wt audit                  # every change, append-only`,
  },
];

const tour: (Omit<TourStep, "code" | "raw"> & { source: string })[] = [
  {
    id: "init",
    title: "Start a worktree",
    summary:
      "A worktree has a root tree; every snapshot captures its working files, with no staging area.",
    file: "terminal",
    source: `wt init acme && cd acme
wt snapshot -m "first snapshot"
wt log -n 3`,
  },
  {
    id: "nested",
    title: "Add a nested tree",
    summary:
      "A sub-directory becomes a tree with its own branches, HEAD, reflog and history.",
    file: "terminal",
    source: `wt tree add api services/api
wt --tree api switch -c feature/auth
wt --tree api snapshot -m "auth: token check"`,
  },
  {
    id: "daemon",
    title: "Let the daemon track",
    summary:
      "worktree-bg takes auto-snapshots after a quiet period and stages them for the team.",
    file: "terminal",
    source: `wt daemon start
wt daemon watch
wt daemon status`,
  },
  {
    id: "share",
    title: "Share and push",
    summary:
      "Staged snapshots show work in progress; pushes send only the objects the server lacks, as deltas.",
    file: "terminal",
    source: `wt register ${DEFAULT_SERVER_URL} --username alice --email alice@example.com
wt remote add origin ${DEFAULT_SERVER_URL} --tenant alice
wt stage && wt status --team
wt push`,
  },
  {
    id: "review",
    title: "Review and merge",
    summary:
      "Merge requests merge on the server by fast-forward, merge snapshot, squash or rebase.",
    file: "terminal",
    source: `wt mr create --source feature/auth --target main --title "Add token auth"
wt mr conflicts 12
wt mr merge 12 --rebase --delete-branch`,
  },
  {
    id: "undo",
    title: "Undo anything local",
    summary: "Every local ref change is logged; undo and redo walk the log.",
    file: "terminal",
    source: `wt undo --list
wt undo
wt redo`,
  },
];

const features: {
  title: string;
  body: string;
  href: string;
  icon: ReactNode;
}[] = [
  {
    title: "Nested trees",
    body: "Independent branches and history per tree in one worktree.",
    href: "/guides/nested-trees",
    icon: <RiGitBranchLine />,
  },
  {
    title: "Staged visibility",
    body: "Labeled work in progress your team sees before you push.",
    href: "/guides/staged-visibility",
    icon: <RiLayoutGridLine />,
  },
  {
    title: "Auto tracking",
    body: "A background daemon snapshots and stages after a quiet period.",
    href: "/guides/auto-tracking-setup",
    icon: <RiFlashlightLine />,
  },
  {
    title: "Sync protocol",
    body: "gRPC over one HTTP/2 port, with packs and binary deltas.",
    href: "/docs/protocol/sync-protocol",
    icon: <RiStackLine />,
  },
  {
    title: "Self-hosted server",
    body: "Tenants, roles, policies, merge requests and an admin panel.",
    href: "/docs/server",
    icon: <RiServerLine />,
  },
  {
    title: "Git interop",
    body: "Import Git history, export snapshots, mirror trees to Git.",
    href: "/guides/migration",
    icon: <RiBookOpenLine />,
  },
];

const internals: { label: string; body: string }[] = [
  {
    label: "BLAKE3 addressing",
    body: "Every blob and manifest is named by the BLAKE3-256 hash of its exact bytes.",
  },
  {
    label: "Deduplication",
    body: "Identical content is stored once; push and pull move only what the other side lacks.",
  },
  {
    label: "Packs and deltas",
    body: "Objects travel in packs with binary deltas against content the receiver already has.",
  },
  {
    label: "Stat cache",
    body: "A per-tree index of size and mtime skips rehashing files that have not changed.",
  },
  {
    label: "One binary server",
    body: "Pure Go, an embedded bbolt database, objects on disk or in S3-compatible storage.",
  },
  {
    label: "Append-only audit",
    body: "Every server change is recorded, streamed live and visible in the admin panel.",
  },
];

const comparisons: Comparison[] = [
  {
    topic: "Workflow",
    git: "An index to stage, and commands like `checkout` that both switch branches and restore files.",
    wt: "A snapshot captures the working tree. `switch`, `restore` and `revert` each do one thing.",
  },
  {
    topic: "Recovery",
    git: "`reset --hard` and force pushes can destroy work and rewrite shared history.",
    wt: "Reverts add snapshots, branches are archived, `wt undo` walks a local operation log, and force pushes need a permission protections can deny.",
  },
  {
    topic: "Collaboration",
    git: "Work stays invisible until it is pushed, so overlaps surface late as conflicts.",
    wt: "Staged snapshots show in-flight work; `wt status --team` and push notes flag overlapping paths.",
  },
  {
    topic: "Access",
    git: "No authentication or authorization of its own; a hosting platform supplies them.",
    wt: "The server enforces tenants, roles, policies with attribute conditions and tree visibility on every call.",
  },
  {
    topic: "Secrets",
    git: "A leaked credential stays in history unless the history is rewritten.",
    wt: "Snapshots are scanned before they are written, and the server scans pushes too.",
  },
  {
    topic: "Many projects",
    git: "Submodules are detached and brittle; monorepos need sparse-checkout rules.",
    wt: "Nested trees live in one worktree with their own history; sparse checkout is built in.",
  },
];

const faqs: Question[] = [
  {
    q: "Is W0rkTree built on Git?",
    a: "No. It has its own object model (BLAKE3-addressed blobs and manifests), its own snapshot history and its own sync protocol (`worktree.sync.v1` over gRPC). Git interoperability comes from `wt git import`, `wt git export` and server-side Git mirrors.",
  },
  {
    q: "What is a nested tree?",
    a: "A directory with its own branches, HEAD, reflog and snapshot history. A worktree always has a root tree; `wt tree add` registers sub-directories as nested trees, and each tree's snapshots exclude the trees nested inside it.",
  },
  {
    q: "What are staged snapshots?",
    a: "Your work in progress, uploaded so teammates can see it with `wt status --team`. They are not branch history: each branch has a default slot and up to ten labeled slots, ended by a push or expired after 30 days.",
  },
  {
    q: "Can I migrate from Git?",
    a: "Yes. `wt git import` converts a Git repository's history into snapshots, and `wt git export` writes snapshots back, so Git-only consumers keep working during a transition.",
  },
  {
    q: "How does multi-tenancy work?",
    a: "Every account has a personal tenant; organizations are separate tenants with members, teams, custom roles and policies. Trees are private, internal or public, and can be forked across tenants.",
  },
  {
    q: "Is it ready for production?",
    a: `Not yet. ${version} is pre-1.0: the CLI, daemon, server and admin panel work end to end, but on-disk and wire formats may still change before 1.0.`,
  },
];

async function highlightAll<T extends { source: string }>(
  items: T[],
  lang: string,
) {
  return Promise.all(
    items.map(async (item) => ({
      ...item,
      code: await highlight(item.source, {
        lang,
        themes: codeThemes,
        defaultColor: false,
      }),
    })),
  );
}

export default async function HomePage() {
  const heroScenes: Scene[] = (await highlightAll(scenes, "bash")).map(
    ({ title, summary, code }) => ({
      title,
      summary,
      code,
    }),
  );
  const steps: TourStep[] = (await highlightAll(tour, "bash")).map(
    ({ source, ...step }) => ({
      ...step,
      raw: source,
    }),
  );

  return (
    <>
      <Band frameClassName="overflow-hidden">
        <HeroGrid />
        <div className={cx("relative pt-14 pb-12 md:pt-20 md:pb-14", gutter)}>
          <Link
            href="/changelog"
            className="animate-float-in group inline-flex items-center gap-2 rounded-full border border-border-button-default bg-background-primary-default py-1 ps-1 pe-3 text-caption-1-medium text-text-secondary shadow-xs transition-colors hover:border-border-button-hover hover:text-text-primary"
          >
            <span className="rounded-full bg-text-primary px-2 py-0.5 font-mono text-background-full">
              v{version}
            </span>
            Read the changelog
            <RiArrowRightLine
              className="size-3.5 transition-transform group-hover:translate-x-0.5"
              aria-hidden
            />
          </Link>

          <h1 className="mt-7 max-w-[15ch] text-[2.5rem] leading-[1.04] font-semibold tracking-[-0.045em] text-balance sm:text-[3.5rem] lg:max-w-[17ch] lg:text-[4.5rem]">
            <WordReveal text="Version control" immediate delay={0.08} />{" "}
            <WordReveal
              text="for trees your whole team can see"
              immediate
              delay={0.2}
              className="text-text-secondary"
            />
          </h1>

          <div className="mt-10 grid gap-8 lg:grid-cols-12 lg:items-end">
            <div className="animate-float-in flex flex-col gap-5 [animation-delay:220ms] lg:col-span-6">
              <p className="text-headline-regular leading-7 text-pretty text-text-secondary md:text-[1.0625rem]">
                W0rkTree is version control with nested trees, automatic change
                tracking and team-visible work in progress, served by a
                self-hosted multi-tenant server. A Rust CLI and daemon, a Go
                server on one port, and Git import and export.
              </p>
              <CreatorCredit />
            </div>
            <div className="animate-float-in flex flex-col gap-4 [animation-delay:300ms] lg:col-span-6 lg:col-start-7 lg:items-end">
              <div className="flex flex-wrap items-center gap-2.5 lg:justify-end">
                <ActionLink
                  href="/docs/installation"
                  trailingIcon={RiArrowRightLine}
                >
                  Get started
                </ActionLink>
                <InstallCommand />
                <ActionLink
                  href={REPO_URL}
                  variant="ghost"
                  trailingIcon={RiArrowRightUpLine}
                  className="bg-transparent"
                >
                  Source
                </ActionLink>
              </div>
              <p className="text-caption-1-regular text-text-secondary">
                Linux, macOS, Windows <Dot /> One server port, :7420 <Dot /> Git
                import and export
              </p>
            </div>
          </div>
        </div>
        <div className={cx("relative pb-10 md:pb-14", gutter)}>
          <HeroScenes scenes={heroScenes} />
        </div>
      </Band>

      <ChapterNav chapters={chapters} />

      <Band>
        <StaggerList
          className="grid grid-cols-1 gap-px bg-separator-border sm:grid-cols-2 lg:grid-cols-3"
          delay={0.05}
        >
          {features.map((feature) => (
            <li key={feature.title} className="bg-background-full">
              <StaggerChild className="h-full">
                <Link
                  href={feature.href}
                  className="group flex h-full items-start gap-4 px-6 py-6 transition-colors hover:bg-background-secondary-default sm:px-8"
                >
                  <span className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-border-button-default bg-surface-raised text-foreground-icon-primary shadow-xs [&>svg]:size-4.5">
                    {feature.icon}
                  </span>
                  <span className="flex min-w-0 flex-col gap-1">
                    <span className="flex items-center gap-1.5 text-body-medium text-text-primary">
                      {feature.title}
                      <RiArrowRightLine
                        className="size-3.5 opacity-0 transition-[opacity,transform] duration-200 group-hover:translate-x-0.5 group-hover:opacity-100"
                        aria-hidden
                      />
                    </span>
                    <span className="text-body-regular leading-6 text-text-secondary">
                      {feature.body}
                    </span>
                  </span>
                </Link>
              </StaggerChild>
            </li>
          ))}
        </StaggerList>
      </Band>

      <SplitExplainer
        id="model"
        index="01"
        eyebrow="Model"
        title="One worktree, many trees"
        lede="A tree is a directory with its own branches, HEAD, reflog and history. Nest them in one worktree instead of juggling submodules or one giant repository."
        points={[
          {
            lead: "Snapshots, not commits.",
            body: "Each snapshot records a tree's whole working state; there is no staging area to forget.",
          },
          {
            lead: "Append-only history.",
            body: "Reverts add snapshots and branches are archived, so nothing is silently lost.",
          },
          {
            lead: "Sparse when you want it.",
            body: "Check out only the paths you need, with objects fetched on demand.",
          },
        ]}
        href="/guides/nested-trees"
        linkLabel="Nested trees guide"
        visual={<TreeDiagram />}
      />

      <StackedExplainer
        id="workflow"
        index="02"
        eyebrow="Workflow"
        title="Every verb does one thing"
        lede={withCode(
          "Snapshot, switch, merge, push: each command has one job and `--json` output, from the first snapshot to a merged review.",
        )}
        points={[
          {
            lead: "Local first.",
            body: withCode(
              "Everything but sharing works offline; `wt sync` queues pushes until the server is back.",
            ),
          },
          {
            lead: "Typed output.",
            body: withCode(
              "Every command prints JSON with `--json`, for scripts and CI.",
            ),
          },
          {
            lead: "Reversible.",
            body: withCode(
              "`wt undo` and `wt redo` walk a log of every local ref change.",
            ),
          },
        ]}
        href="/docs/cli"
        linkLabel="CLI reference"
        visual={<CodeTourView steps={steps} />}
        sunken
      />

      <SplitExplainer
        id="team"
        index="03"
        eyebrow="Team"
        title="See work before it is pushed"
        lede="Staged snapshots upload work in progress without touching branch history, so overlaps show up while they are still cheap to resolve."
        points={[
          {
            lead: "Labeled slots.",
            body: "Keep several pieces of work in progress per branch, each under its own label.",
          },
          {
            lead: "Overlap notes.",
            body: "Pushing tells you who else has staged changes to the same paths.",
          },
          {
            lead: "Check out a teammate's state.",
            body: withCode(
              "`wt checkout --staged` opens it as a local branch.",
            ),
          },
        ]}
        href="/guides/staged-visibility"
        linkLabel="Staged visibility guide"
        visual={<TeamStatus />}
        reverse
      />

      <SplitExplainer
        id="server"
        index="04"
        eyebrow="Server"
        title="One server, one port, many tenants"
        lede="The Go server serves sync, the REST API, live events and the admin panel on one port, with tenants, teams, roles and policies enforced on every call."
        points={[
          {
            lead: "Fine-grained access.",
            body: "Built-in and custom roles, per-tree grants, and deny-overrides policies with attribute conditions.",
          },
          {
            lead: "Sign-in options.",
            body: "Passwords with MFA, single sign-on, device sign-in for the CLI, scoped API tokens.",
          },
          {
            lead: "Operable.",
            body: "Backups to disk or S3, retention, Prometheus metrics, Helm chart and container images.",
          },
        ]}
        href="/docs/server"
        linkLabel="Server documentation"
        visual={<ServerStack />}
        sunken
      />

      <Band id="governance">
        <div className={cx("py-16 md:py-20", gutter)}>
          <div className="grid gap-8 lg:grid-cols-12">
            <SectionHeading
              index="05"
              eyebrow="Governance"
              title="Rules the server enforces"
              className="lg:col-span-6"
            >
              Protections, licenses and scanning run on every push and merge,
              with every decision written to an append-only audit log.
            </SectionHeading>
            <div className="flex flex-col justify-end gap-3 lg:col-span-5 lg:col-start-8">
              <TextLink
                href="/docs/server/branch-protection"
                trailingIcon={RiArrowRightLine}
              >
                Branch protection
              </TextLink>
              <TextLink
                href="/docs/server/secret-scanning"
                trailingIcon={RiArrowRightLine}
              >
                Secret scanning
              </TextLink>
            </div>
          </div>
          <Reveal className="mt-12">
            <ul className="grid gap-px overflow-hidden rounded-2xl border border-border-button-default bg-separator-border sm:grid-cols-2 lg:grid-cols-4">
              {[
                [
                  "Branch protection",
                  "Required approvals, CI checks, signed and linear history, no force pushes.",
                ],
                [
                  "License rules",
                  "Per-path SPDX rules with read, modify and redistribute grants.",
                ],
                [
                  "Secret scanning",
                  "Pushes are scanned and can be blocked; findings can be allowlisted.",
                ],
                [
                  "Config as code",
                  ".wt-tree files declare branches, protections and access, validated on push.",
                ],
              ].map(([title, body], index) => (
                <li
                  key={title}
                  className="flex flex-col gap-2 bg-background-full p-6"
                >
                  <span className="font-mono text-caption-2-regular text-text-tertiary tabular-nums">
                    {String(index + 1).padStart(2, "0")}
                  </span>
                  <span className="text-body-medium text-text-primary">
                    {title}
                  </span>
                  <span className="text-body-regular leading-6 text-text-secondary">
                    {body}
                  </span>
                </li>
              ))}
            </ul>
          </Reveal>
        </div>
      </Band>

      <Band id="internals" frameClassName="bg-surface-sunken">
        <div className={cx("py-16 md:py-20", gutter)}>
          <SectionHeading
            index="06"
            eyebrow="Internals"
            title="Content-addressed all the way down"
          >
            The same object model on your machine and on the server, so sync is
            a matter of sending what is missing.
          </SectionHeading>
          <Reveal className="mt-12">
            <ul className="grid gap-px overflow-hidden rounded-2xl border border-border-button-default bg-separator-border sm:grid-cols-2 lg:grid-cols-3">
              {internals.map((item) => (
                <li
                  key={item.label}
                  className="flex flex-col gap-2 bg-surface-raised p-6"
                >
                  <RiFingerprintLine
                    className="size-4 text-foreground-icon-secondary"
                    aria-hidden
                  />
                  <span className="text-body-medium text-text-primary">
                    {item.label}
                  </span>
                  <span className="text-body-regular leading-6 text-text-secondary">
                    {item.body}
                  </span>
                </li>
              ))}
            </ul>
          </Reveal>
        </div>
      </Band>

      <Band id="compare">
        <div className={cx("py-16 md:py-20", gutter)}>
          <SectionHeading index="07" eyebrow="Compare" title="Coming from Git">
            What changes when the version control system and the server are
            designed together.
          </SectionHeading>
          <Reveal className="mt-12">
            <CompareTable rows={comparisons} />
          </Reveal>
        </div>
      </Band>

      <Band id="faq" frameClassName="bg-surface-sunken">
        <div
          className={cx("grid gap-10 py-16 md:py-20 lg:grid-cols-12", gutter)}
        >
          <SectionHeading
            index="08"
            eyebrow="FAQ"
            title="Questions"
            className="lg:col-span-4"
          >
            Anything else is in the documentation or the issue tracker.
          </SectionHeading>
          <Reveal className="lg:col-span-8">
            <FaqList items={faqs} />
          </Reveal>
        </div>
      </Band>

      <Band frameClassName="overflow-hidden">
        <div
          className="hatch hatch-drift pointer-events-none absolute inset-0 opacity-60"
          aria-hidden
        />
        <div
          className={cx(
            "relative flex flex-col items-start gap-6 py-16 md:flex-row md:items-end md:justify-between md:py-20",
            gutter,
          )}
        >
          <div className="flex max-w-xl flex-col gap-3">
            <p className="eyebrow">Get started</p>
            <h2 className="text-display-4-semibold tracking-[-0.025em] text-balance md:text-display-3-semibold">
              Start with one tree.
            </h2>
            <p className="text-headline-regular leading-7 text-text-secondary">
              Install the CLI, take a snapshot, and add the server when your
              team is ready.
            </p>
          </div>
          <div className="flex flex-wrap gap-2.5">
            <ActionLink
              href="/guides/quick-start"
              trailingIcon={RiArrowRightLine}
            >
              Quick start
            </ActionLink>
            <ActionLink href="/docs" variant="secondary">
              Read the docs
            </ActionLink>
          </div>
        </div>
      </Band>
    </>
  );
}

function Dot() {
  return (
    <span className="mx-1.5 inline-block size-[3px] rounded-full bg-text-tertiary align-middle" />
  );
}
