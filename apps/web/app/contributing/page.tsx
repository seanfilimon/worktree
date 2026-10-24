import Link from "next/link";
import type { Metadata } from "next";
import { Badge } from "@/components/ui/badge";

export const metadata: Metadata = {
  title: "Contributing - W0rkTree",
  description:
    "How to contribute to W0rkTree — setup, conventions, and PR process.",
};

export default function ContributingPage() {
  return (
    <div className="container mx-auto max-w-4xl px-6 py-16">
      {/* Hero */}
      <section className="mb-16 text-center">
        <h1 className="mb-4 text-4xl font-bold tracking-tight sm:text-5xl">
          Contributing to W0rkTree
        </h1>
        <p className="mx-auto max-w-2xl text-lg text-muted-foreground">
          W0rkTree is open source and we welcome contributions of all kinds.
          Whether you&apos;re fixing a typo, improving docs, or building a new
          feature — every contribution matters.
        </p>
      </section>

      {/* Getting Started */}
      <section className="mb-14">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">
          Getting Started
        </h2>
        <p className="mb-4 text-muted-foreground">
          Before you begin, make sure you have the following prerequisites
          installed on your machine:
        </p>
        <ul className="mb-4 list-inside list-disc space-y-2 text-muted-foreground">
          <li>
            <strong className="text-foreground">Rust toolchain</strong> —
            install via{" "}
            <a
              href="https://rustup.rs"
              target="_blank"
              rel="noopener noreferrer"
              className="underline underline-offset-4 hover:text-foreground"
            >
              rustup.rs
            </a>
          </li>
          <li>
            <strong className="text-foreground">Node.js 18+</strong> — we
            recommend using{" "}
            <a
              href="https://github.com/nvm-sh/nvm"
              target="_blank"
              rel="noopener noreferrer"
              className="underline underline-offset-4 hover:text-foreground"
            >
              nvm
            </a>{" "}
            to manage versions
          </li>
          <li>
            <strong className="text-foreground">npm</strong> — our package
            manager of choice for the monorepo
          </li>
        </ul>
        <p className="text-muted-foreground">
          Once those are set up, clone the repo and build the project to verify
          everything works.
        </p>
      </section>

      {/* Development Setup */}
      <section className="mb-14">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">
          Development Setup
        </h2>
        <p className="mb-4 text-muted-foreground">
          Follow these steps to get a local development environment running:
        </p>
        <div className="space-y-4">
          <div>
            <p className="mb-2 text-sm font-medium text-foreground">
              1. Clone the repository
            </p>
            <pre className="overflow-x-auto rounded-lg border bg-muted/50 px-4 py-3 text-sm">
              <code>
                git clone https://github.com/W0rkTree/worktree.git{"\n"}cd
                worktree
              </code>
            </pre>
          </div>
          <div>
            <p className="mb-2 text-sm font-medium text-foreground">
              2. Build the Rust workspace
            </p>
            <pre className="overflow-x-auto rounded-lg border bg-muted/50 px-4 py-3 text-sm">
              <code>cargo build</code>
            </pre>
          </div>
          <div>
            <p className="mb-2 text-sm font-medium text-foreground">
              3. Install JavaScript dependencies
            </p>
            <pre className="overflow-x-auto rounded-lg border bg-muted/50 px-4 py-3 text-sm">
              <code>npm install</code>
            </pre>
          </div>
          <div>
            <p className="mb-2 text-sm font-medium text-foreground">
              4. Start the docs site in development mode
            </p>
            <pre className="overflow-x-auto rounded-lg border bg-muted/50 px-4 py-3 text-sm">
              <code>npm run dev</code>
            </pre>
          </div>
        </div>
        <p className="mt-4 text-sm text-muted-foreground">
          The docs site will be available at{" "}
          <code className="rounded bg-muted px-1.5 py-0.5 text-xs">
            http://localhost:3000
          </code>
          .
        </p>
      </section>

      {/* Code Style */}
      <section className="mb-14">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">
          Code Style
        </h2>
        <p className="mb-4 text-muted-foreground">
          Consistent code style keeps the project maintainable. Please follow
          these guidelines:
        </p>
        <div className="space-y-4">
          <div className="rounded-lg border p-4">
            <h3 className="mb-1 font-medium">Rust</h3>
            <p className="text-sm text-muted-foreground">
              Follow{" "}
              <code className="rounded bg-muted px-1.5 py-0.5 text-xs">
                rustfmt
              </code>{" "}
              for formatting and{" "}
              <code className="rounded bg-muted px-1.5 py-0.5 text-xs">
                clippy
              </code>{" "}
              for linting. Run{" "}
              <code className="rounded bg-muted px-1.5 py-0.5 text-xs">
                cargo fmt --check
              </code>{" "}
              and{" "}
              <code className="rounded bg-muted px-1.5 py-0.5 text-xs">
                cargo clippy
              </code>{" "}
              before pushing.
            </p>
          </div>
          <div className="rounded-lg border p-4">
            <h3 className="mb-1 font-medium">TypeScript</h3>
            <p className="text-sm text-muted-foreground">
              Use ESLint + Prettier. Run{" "}
              <code className="rounded bg-muted px-1.5 py-0.5 text-xs">
                npm run lint
              </code>{" "}
              and{" "}
              <code className="rounded bg-muted px-1.5 py-0.5 text-xs">
                npm run format
              </code>{" "}
              to check and fix issues automatically.
            </p>
          </div>
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
              <code className="rounded bg-muted px-1.5 py-0.5 text-xs">
                feat:
              </code>
              ,{" "}
              <code className="rounded bg-muted px-1.5 py-0.5 text-xs">
                fix:
              </code>
              ,{" "}
              <code className="rounded bg-muted px-1.5 py-0.5 text-xs">
                docs:
              </code>
              , or{" "}
              <code className="rounded bg-muted px-1.5 py-0.5 text-xs">
                chore:
              </code>
              .
            </p>
          </div>
        </div>
      </section>

      {/* Pull Request Process */}
      <section className="mb-14">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">
          Pull Request Process
        </h2>
        <p className="mb-4 text-muted-foreground">
          We follow a standard fork-and-PR workflow:
        </p>
        <ol className="mb-4 list-inside list-decimal space-y-3 text-muted-foreground">
          <li>
            <strong className="text-foreground">Fork</strong> — fork the
            repository to your GitHub account.
          </li>
          <li>
            <strong className="text-foreground">Branch</strong> — create a
            feature branch from{" "}
            <code className="rounded bg-muted px-1.5 py-0.5 text-xs">main</code>{" "}
            (e.g.{" "}
            <code className="rounded bg-muted px-1.5 py-0.5 text-xs">
              feat/my-feature
            </code>
            ).
          </li>
          <li>
            <strong className="text-foreground">Code</strong> — make your
            changes, keeping commits focused and well-described.
          </li>
          <li>
            <strong className="text-foreground">Test</strong> — run the full
            test suite with{" "}
            <code className="rounded bg-muted px-1.5 py-0.5 text-xs">
              cargo test
            </code>{" "}
            and{" "}
            <code className="rounded bg-muted px-1.5 py-0.5 text-xs">
              npm test
            </code>{" "}
            to make sure nothing is broken.
          </li>
          <li>
            <strong className="text-foreground">PR</strong> — open a pull
            request against{" "}
            <code className="rounded bg-muted px-1.5 py-0.5 text-xs">main</code>
            . Fill out the PR template and link any related issues.
          </li>
        </ol>
        <p className="text-muted-foreground">
          All PRs must pass CI checks (linting, formatting, tests) before
          merging. A maintainer will review your PR, provide feedback if needed,
          and merge it once approved.
        </p>
      </section>

      {/* Issue Labels */}
      <section className="mb-14">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">
          Issue Labels
        </h2>
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
        <p className="mt-4 text-sm text-muted-foreground">
          If you&apos;re looking for a place to start, filter issues by{" "}
          <Badge variant="outline" className="mx-1 text-xs">
            good-first-issue
          </Badge>{" "}
          or{" "}
          <Badge variant="outline" className="mx-1 text-xs">
            help-wanted
          </Badge>
          .
        </p>
      </section>

      {/* Code of Conduct */}
      <section className="mb-14">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">
          Code of Conduct
        </h2>
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

      {/* Questions */}
      <section className="mb-8">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">
          Questions?
        </h2>
        <p className="text-muted-foreground">
          If you have questions, need help getting started, or want to discuss
          an idea before working on it, join us on{" "}
          <a
            href="https://discord.gg/worktree"
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
