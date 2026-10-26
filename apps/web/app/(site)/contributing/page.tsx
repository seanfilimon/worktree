import type { ReactNode } from "react";
import Link from "next/link";
import type { Metadata } from "next";
import {
  RiArrowRightUpLine,
  RiDiscordLine,
  RiGithubLine,
} from "react-icons/ri";
import { DiagramFrame } from "@/components/home/frame";
import { Reveal } from "@/components/home/motion";
import { CellAnchor, CellArrow, HoverHatch, StepNumber } from "@/components/project/cells";
import { CopyButton } from "@/components/project/copy-button";
import { ActionLink } from "@/components/site/action";
import { Band, SectionHeading, gutter } from "@/components/site/frame";
import { StaggerChild, StaggerList } from "@/components/site/motion-text";
import { PageHero } from "@/components/site/page-hero";
import { CONTRIBUTING_URL, DISCORD_URL, REPO_URL } from "@/lib/site";
import { cx } from "@/utils/cx";

export const metadata: Metadata = {
  title: "Contributing",
  description:
    "How to contribute to W0rkTree — setup, conventions, and PR process.",
};

const prerequisites = [
  { name: "Rust (stable)", note: "for the CLI, SDK, daemon and Git bridge", href: "https://rustup.rs" },
  { name: "Go 1.25+", note: "for the server in services/server-go", href: "https://go.dev/dl/" },
  { name: "Node.js 22+ and npm", note: "for the admin panel and this website", href: "https://nodejs.org/" },
];

const setupSteps = [
  { title: "Clone the repository", command: `git clone ${REPO_URL}.git\ncd worktree` },
  { title: "Build and test the Rust workspace", command: "cargo build --workspace\ncargo test --workspace" },
  {
    title: "Build and test the Go server",
    command: "cd services/server-go\ngo build ./... && go vet ./... && go test ./...",
  },
  { title: "Build and test the admin panel", command: "cd apps/admin\nnpm install\nnpm run build && npm test" },
  { title: "Run the end-to-end suite", command: "bash scripts/e2e.sh" },
  { title: "Work on this website", command: "cd apps/web\nnpm install\nnpm run dev" },
];

const styleRules: { title: string; text: ReactNode }[] = [
  {
    title: "Rust",
    text: "Edition 2021. Code must pass cargo fmt and cargo clippy --workspace --all-targets -- -D warnings. No unwrap() on I/O in library code.",
  },
  {
    title: "Go",
    text: "gofmt and go vet clean, table-driven tests using t.TempDir(). Pure Go only (no cgo), with the dependency allow-list from the engineering plan.",
  },
  {
    title: "TypeScript",
    text: "Strict TypeScript. Run npm run lint and npm run typecheck in the app you changed before opening a PR.",
  },
  {
    title: "Windows",
    text: "Windows is a first-class platform: no hard-coded /tmp, handle backslash paths, and tolerate atomic renames that fail because the target exists.",
  },
];

const labels = ["bug", "feature", "docs", "good-first-issue", "help-wanted"];

const inlineCode =
  "rounded-md border border-separator-border bg-background-secondary-default px-1.5 py-px font-mono text-[0.86em] text-text-primary";

const inlineLink =
  "text-text-primary underline decoration-text-tertiary underline-offset-4 transition-colors hover:decoration-current";

function Code({ children }: { children: ReactNode }) {
  return <code className={inlineCode}>{children}</code>;
}

function ExternalLink({ href, children }: { href: string; children: ReactNode }) {
  return (
    <a href={href} target="_blank" rel="noopener noreferrer" className={inlineLink}>
      {children}
    </a>
  );
}

/** A shell command in the landing page's diagram frame, with a copy button and a prompt on each line. */
function CommandFrame({ command }: { command: string }) {
  return (
    <DiagramFrame
      label="Terminal"
      actions={<CopyButton text={command} />}
      bodyClassName="p-0 sm:p-0"
    >
      <pre className="overflow-x-auto px-4 py-4 font-mono text-body-2-regular leading-6 sm:px-5">
        <code>
          {command.split("\n").map((line, index) => (
            <span key={index} className="block">
              <span aria-hidden className="select-none text-text-tertiary">
                ${" "}
              </span>
              {line}
            </span>
          ))}
        </code>
      </pre>
    </DiagramFrame>
  );
}

const prSteps: { title: string; body: ReactNode }[] = [
  { title: "Fork", body: "Fork the repository to your GitHub account." },
  {
    title: "Branch",
    body: (
      <>
        Create a feature branch from <Code>main</Code> (e.g. <Code>feat/my-feature</Code>).
      </>
    ),
  },
  {
    title: "Code",
    body: (
      <>
        Make your changes, keeping commits focused and well-described. Interfaces in{" "}
        <Code>docs/ENGINEERING_PLAN.md</Code> are contracts: keep changes to them additive.
      </>
    ),
  },
  {
    title: "Test",
    body: "Run the build, lint and test commands above for every component you touched.",
  },
  {
    title: "PR",
    body: (
      <>
        Open a pull request against <Code>main</Code> and link any related issues.
      </>
    ),
  },
];

export default function ContributingPage() {
  return (
    <>
      <PageHero
        eyebrow="Contributing"
        title="Contributing to W0rkTree."
        muted="Setup, conventions, and PR process."
        actions={
          <>
            <ActionLink
              href={CONTRIBUTING_URL}
              trailingIcon={RiArrowRightUpLine}
              target="_blank"
              rel="noopener noreferrer"
            >
              CONTRIBUTING
            </ActionLink>
            <ActionLink
              href={REPO_URL}
              variant="secondary"
              leadingIcon={RiGithubLine}
              target="_blank"
              rel="noopener noreferrer"
            >
              Repository
            </ActionLink>
          </>
        }
      >
        W0rkTree is open source and we welcome contributions of all kinds. Whether you&apos;re fixing
        a typo, improving docs, or building a new feature — every contribution matters. The
        repository&apos;s <ExternalLink href={CONTRIBUTING_URL}>CONTRIBUTING</ExternalLink> file is
        the authoritative version of this page.
      </PageHero>

      {/* 01: what to install, as a row of outbound cells. */}
      <Band id="prerequisites">
        <div className={cx("py-14 md:py-16", gutter)}>
          <SectionHeading index="01" eyebrow="Prerequisites" title="Prerequisites">
            W0rkTree is a polyglot monorepo: a Rust workspace in <Code>crates/</Code>, a Go server in{" "}
            <Code>services/server-go</Code> and TypeScript apps in <Code>apps/</Code>.
          </SectionHeading>
        </div>
        <StaggerList className="grid gap-px border-t border-separator-border bg-separator-border md:grid-cols-3">
          {prerequisites.map((item) => (
            <li key={item.name} className="bg-background-full">
              <StaggerChild className="h-full">
                <CellAnchor href={item.href}>
                  <HoverHatch />
                  <span className="relative flex items-center gap-1.5 text-headline-medium">
                    {item.name}
                    <CellArrow external />
                  </span>
                  <span className="relative text-body-regular leading-6 text-text-secondary">
                    {item.note}
                  </span>
                  <span className="relative mt-auto pt-4 font-mono text-caption-1-regular text-text-secondary transition-colors duration-300 group-hover:text-text-primary">
                    {item.href.replace(/^https:\/\//, "").replace(/\/$/, "")}
                  </span>
                </CellAnchor>
              </StaggerChild>
            </li>
          ))}
        </StaggerList>
      </Band>

      {/* 02: numbered setup steps, each beside its commands. */}
      <Band id="setup" frameClassName="bg-surface-sunken">
        <div className={cx("py-14 md:py-16", gutter)}>
          <SectionHeading index="02" eyebrow="Setup" title="Development Setup" />
        </div>
        <ol className="border-t border-separator-border">
          {setupSteps.map((step, index) => (
            <li
              key={step.title}
              className={cx(
                "grid gap-4 border-b border-separator-border py-8 last:border-b-0 lg:grid-cols-12 lg:gap-10",
                gutter,
              )}
            >
              <div className="flex items-baseline gap-3 lg:col-span-4 lg:flex-col lg:gap-2 lg:pt-3">
                <StepNumber value={index + 1} />
                <h3 className="text-headline-medium">{step.title}</h3>
              </div>
              <Reveal className="min-w-0 lg:col-span-8">
                <CommandFrame command={step.command} />
              </Reveal>
            </li>
          ))}
        </ol>
        <p
          className={cx(
            "border-t border-separator-border py-5 text-body-regular leading-6 text-text-secondary",
            gutter,
          )}
        >
          The website runs at <Code>http://localhost:3000</Code>. The{" "}
          <Link href="/docs/installation" className={inlineLink}>
            installation guide
          </Link>{" "}
          covers each component in more detail.
        </p>
      </Band>

      {/* 03: one cell per language, the commit convention, and a hatched filler. */}
      <Band id="code-style">
        <div className={cx("py-14 md:py-16", gutter)}>
          <SectionHeading index="03" eyebrow="Conventions" title="Code Style" />
        </div>
        <StaggerList className="grid gap-px border-t border-separator-border bg-separator-border md:grid-cols-2 lg:grid-cols-3">
          {styleRules.map((rule) => (
            <li key={rule.title} className="bg-background-full">
              <StaggerChild className="flex h-full flex-col gap-2 p-6 sm:p-8">
                <h3 className="text-headline-medium">{rule.title}</h3>
                <p className="text-body-regular leading-6 text-pretty text-text-secondary">{rule.text}</p>
              </StaggerChild>
            </li>
          ))}
          <li className="bg-background-full">
            <StaggerChild className="flex h-full flex-col gap-2 p-6 sm:p-8">
              <h3 className="text-headline-medium">Commit Messages</h3>
              <p className="text-body-regular leading-6 text-pretty text-text-secondary">
                We use <ExternalLink href="https://www.conventionalcommits.org/">Conventional Commits</ExternalLink>
                . Prefix your messages with a type such as <Code>feat:</Code>, <Code>fix:</Code>,{" "}
                <Code>docs:</Code>, or <Code>chore:</Code>.
              </p>
            </StaggerChild>
          </li>
          <li aria-hidden className="hatch hidden bg-background-full lg:block" />
        </StaggerList>
      </Band>

      {/* 04: the fork-and-PR workflow as five numbered columns. */}
      <Band id="pull-requests" frameClassName="bg-surface-sunken">
        <div className={cx("py-14 md:py-16", gutter)}>
          <SectionHeading index="04" eyebrow="Workflow" title="Pull Request Process">
            We follow a standard fork-and-PR workflow:
          </SectionHeading>
        </div>
        <StaggerList className="grid gap-px border-t border-separator-border bg-separator-border sm:grid-cols-2 lg:grid-cols-5">
          {prSteps.map((step, index) => (
            <li key={step.title} className="bg-surface-sunken">
              <StaggerChild className="flex h-full flex-col gap-2 p-6">
                <StepNumber value={index + 1} className="mb-4" />
                <h3 className="text-headline-medium">{step.title}</h3>
                <p className="text-body-regular leading-6 text-pretty text-text-secondary">
                  {step.body}
                </p>
              </StaggerChild>
            </li>
          ))}
        </StaggerList>
        <p
          className={cx(
            "border-t border-separator-border py-5 text-body-regular leading-6 text-text-secondary",
            gutter,
          )}
        >
          A maintainer will review your PR, provide feedback if needed, and merge it once approved.
        </p>
      </Band>

      {/* 05: labels, conduct, and where to ask. */}
      <Band id="more" marks={false}>
        <div className="grid gap-px bg-separator-border lg:grid-cols-3">
          <section className="flex flex-col gap-4 bg-background-full p-6 sm:p-8" aria-labelledby="labels-heading">
            <p className="eyebrow">05 · Labels</p>
            <h2 id="labels-heading" className="text-title-3-semibold">
              Issue Labels
            </h2>
            <p className="text-body-regular leading-6 text-text-secondary">
              We use labels to categorize issues. Here are the ones you&apos;ll encounter most often:
            </p>
            <ul className="flex flex-wrap gap-2">
              {labels.map((label, index) => (
                <li
                  key={label}
                  className={cx(
                    "rounded-md border px-2 py-0.5 font-mono text-caption-1-regular",
                    index === 0
                      ? "border-text-primary bg-text-primary text-background-full"
                      : index < 3
                        ? "border-border-button-default bg-background-secondary-default text-text-primary"
                        : "border-dashed border-border-button-default text-text-secondary",
                  )}
                >
                  {label}
                </li>
              ))}
            </ul>
          </section>
          <section className="flex flex-col gap-4 bg-background-full p-6 sm:p-8" aria-labelledby="conduct-heading">
            <p className="eyebrow">06 · Conduct</p>
            <h2 id="conduct-heading" className="text-title-3-semibold">
              Code of Conduct
            </h2>
            <p className="text-body-regular leading-6 text-text-secondary">
              All contributors are expected to follow our{" "}
              <Link href="/code-of-conduct" className={inlineLink}>
                Code of Conduct
              </Link>
              . We are committed to providing a welcoming and inclusive environment for everyone.
              Please read it before participating.
            </p>
          </section>
          <section
            className="relative flex flex-col items-start gap-4 overflow-hidden bg-background-full p-6 sm:p-8"
            aria-labelledby="questions-heading"
          >
            <div aria-hidden className="hatch hatch-drift pointer-events-none absolute inset-0 opacity-60" />
            <p className="eyebrow relative">07 · Questions</p>
            <h2 id="questions-heading" className="relative text-title-3-semibold">
              Questions?
            </h2>
            <p className="relative text-body-regular leading-6 text-text-secondary">
              If you have questions, need help getting started, or want to discuss an idea before
              working on it, join us on <ExternalLink href={DISCORD_URL}>Discord</ExternalLink>.
              We&apos;re happy to help!
            </p>
            <ActionLink
              href={DISCORD_URL}
              variant="secondary"
              leadingIcon={RiDiscordLine}
              className="relative mt-auto"
              target="_blank"
              rel="noopener noreferrer"
            >
              Join the Discord
            </ActionLink>
          </section>
        </div>
      </Band>
    </>
  );
}
