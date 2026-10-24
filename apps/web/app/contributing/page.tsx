import Link from "next/link";
import type { Metadata } from "next";
import { Badge } from "@/components/ui/badge";
import { CONTRIBUTING_URL, DISCORD_URL, REPO_URL } from "@/lib/site";

export const metadata: Metadata = {
  title: "Contributing - W0rkTree",
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

const styleRules = [
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

function CommandBlock({ command }: { command: string }) {
  return (
    <pre className="overflow-x-auto rounded-lg border bg-muted/50 px-4 py-3 text-sm">
      <code>{command}</code>
    </pre>
  );
}

const inlineCode = "rounded bg-muted px-1.5 py-0.5 text-xs";

export default function ContributingPage() {
  return (
    <div className="container mx-auto max-w-4xl px-6 py-16">
      <section className="mb-16 text-center">
        <h1 className="mb-4 text-4xl font-bold tracking-tight sm:text-5xl">
          Contributing to W0rkTree
        </h1>
        <p className="mx-auto max-w-2xl text-lg text-muted-foreground">
          W0rkTree is open source and we welcome contributions of all kinds.
          Whether you&apos;re fixing a typo, improving docs, or building a new
          feature — every contribution matters. The repository&apos;s{" "}
          <a
            href={CONTRIBUTING_URL}
            target="_blank"
            rel="noopener noreferrer"
            className="underline underline-offset-4 hover:text-foreground"
          >
            CONTRIBUTING
          </a>{" "}
          file is the authoritative version of this page.
        </p>
      </section>

      <section className="mb-14">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">Prerequisites</h2>
        <p className="mb-4 text-muted-foreground">
          W0rkTree is a polyglot monorepo: a Rust workspace in{" "}
          <code className={inlineCode}>crates/</code>, a Go server in{" "}
          <code className={inlineCode}>services/server-go</code> and TypeScript
          apps in <code className={inlineCode}>apps/</code>.
        </p>
        <ul className="list-inside list-disc space-y-2 text-muted-foreground">
          {prerequisites.map((item) => (
            <li key={item.name}>
              <a
                href={item.href}
                target="_blank"
                rel="noopener noreferrer"
                className="font-medium text-foreground underline underline-offset-4"
              >
                {item.name}
              </a>{" "}
              — {item.note}
            </li>
          ))}
        </ul>
      </section>

      <section className="mb-14">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">Development Setup</h2>
        <ol className="space-y-4">
          {setupSteps.map((step, index) => (
            <li key={step.title}>
              <p className="mb-2 text-sm font-medium text-foreground">
                {index + 1}. {step.title}
              </p>
              <CommandBlock command={step.command} />
            </li>
          ))}
        </ol>
        <p className="mt-4 text-sm text-muted-foreground">
          The website runs at{" "}
          <code className={inlineCode}>http://localhost:3000</code>. The{" "}
          <Link href="/docs/installation" className="underline underline-offset-4 hover:text-foreground">
            installation guide
          </Link>{" "}
          covers each component in more detail.
        </p>
      </section>

      <section className="mb-14">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">Code Style</h2>
        <div className="space-y-4">
          {styleRules.map((rule) => (
            <div key={rule.title} className="rounded-lg border p-4">
              <h3 className="mb-1 font-medium">{rule.title}</h3>
              <p className="text-sm text-muted-foreground">{rule.text}</p>
            </div>
          ))}
          <div className="rounded-lg border p-4">
            <h3 className="mb-1 font-medium">Commit Messages</h3>
            <p className="text-sm text-muted-foreground">
              We use{" "}
              <a
                href="https://www.conventionalcommits.org/"
                target="_blank"
                rel="noopener noreferrer"
                className="underline underline-offset-4 hover:text-foreground"
              >
                Conventional Commits
              </a>
              . Prefix your messages with a type such as{" "}
              <code className={inlineCode}>feat:</code>,{" "}
              <code className={inlineCode}>fix:</code>,{" "}
              <code className={inlineCode}>docs:</code>, or{" "}
              <code className={inlineCode}>chore:</code>.
            </p>
          </div>
        </div>
      </section>

      <section className="mb-14">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">Pull Request Process</h2>
        <p className="mb-4 text-muted-foreground">We follow a standard fork-and-PR workflow:</p>
        <ol className="mb-4 list-inside list-decimal space-y-3 text-muted-foreground">
          <li>
            <strong className="text-foreground">Fork</strong> — fork the
            repository to your GitHub account.
          </li>
          <li>
            <strong className="text-foreground">Branch</strong> — create a
            feature branch from <code className={inlineCode}>main</code> (e.g.{" "}
            <code className={inlineCode}>feat/my-feature</code>).
          </li>
          <li>
            <strong className="text-foreground">Code</strong> — make your
            changes, keeping commits focused and well-described. Interfaces in{" "}
            <code className={inlineCode}>docs/ENGINEERING_PLAN.md</code> are
            contracts: keep changes to them additive.
          </li>
          <li>
            <strong className="text-foreground">Test</strong> — run the build,
            lint and test commands above for every component you touched.
          </li>
          <li>
            <strong className="text-foreground">PR</strong> — open a pull
            request against <code className={inlineCode}>main</code> and link
            any related issues.
          </li>
        </ol>
        <p className="text-muted-foreground">
          A maintainer will review your PR, provide feedback if needed, and
          merge it once approved.
        </p>
      </section>

      <section className="mb-14">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">Issue Labels</h2>
        <p className="mb-4 text-muted-foreground">
          We use labels to categorize issues. Here are the ones you&apos;ll
          encounter most often:
        </p>
        <div className="flex flex-wrap gap-3">
          <Badge variant="destructive">bug</Badge>
          <Badge variant="default">feature</Badge>
          <Badge variant="secondary">docs</Badge>
          <Badge variant="outline">good-first-issue</Badge>
          <Badge variant="outline">help-wanted</Badge>
        </div>
      </section>

      <section className="mb-14">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">Code of Conduct</h2>
        <p className="text-muted-foreground">
          All contributors are expected to follow our{" "}
          <Link
            href="/code-of-conduct"
            className="font-medium text-foreground underline underline-offset-4 hover:text-primary"
          >
            Code of Conduct
          </Link>
          . We are committed to providing a welcoming and inclusive environment
          for everyone. Please read it before participating.
        </p>
      </section>

      <section className="mb-8">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">Questions?</h2>
        <p className="text-muted-foreground">
          If you have questions, need help getting started, or want to discuss
          an idea before working on it, join us on{" "}
          <a
            href={DISCORD_URL}
            target="_blank"
            rel="noopener noreferrer"
            className="font-medium text-foreground underline underline-offset-4 hover:text-primary"
          >
            Discord
          </a>
          . We&apos;re happy to help!
        </p>
      </section>
    </div>
  );
}
