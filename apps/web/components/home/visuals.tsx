import type { ReactNode } from "react";
import { RiAddLine } from "react-icons/ri";
import { cx } from "@/utils/cx";
import { DiagramFrame } from "./frame";

/** Renders `backtick` spans in copy as inline code. */
export function withCode(text: string): ReactNode[] {
  return text.split("`").map((part, index) =>
    index % 2 === 1 ? (
      <code
        key={index}
        className="rounded-md bg-background-secondary-default px-1.5 py-0.5 font-mono text-[0.85em] text-text-primary"
      >
        {part}
      </code>
    ) : (
      part
    ),
  );
}

interface TreeRow {
  depth: number;
  name: string;
  /** A tree root: its own branches, HEAD and history. */
  tree?: { name: string; branch: string };
  note?: string;
}

const rows: TreeRow[] = [
  { depth: 0, name: "acme/", tree: { name: "acme", branch: "main" } },
  { depth: 1, name: ".wt/", note: "objects, snapshots, refs, reflog" },
  {
    depth: 1,
    name: "services/api/",
    tree: { name: "api", branch: "feature/auth" },
  },
  { depth: 2, name: "src/auth.rs" },
  { depth: 1, name: "apps/web/", tree: { name: "web", branch: "main" } },
  { depth: 2, name: "app/login.tsx" },
  { depth: 1, name: "docs/", note: "belongs to the root tree" },
];

/** One worktree, three trees: each nested tree has its own branch, shown beside it. */
export function TreeDiagram() {
  return (
    <DiagramFrame
      label="wt tree list"
      footer={withCode(
        "Each tree snapshots only its own files: the root tree excludes `services/api/` and `apps/web/`, which have their own history.",
      )}
    >
      <ul className="flex flex-col font-mono text-[0.8125rem] leading-7">
        {rows.map((row) => (
          <li key={row.name} className="flex items-center gap-3">
            <span
              className="flex min-w-0 items-center gap-2"
              style={{ paddingInlineStart: `${row.depth * 1.25}rem` }}
            >
              {row.depth > 0 ? (
                <span
                  aria-hidden
                  className="h-px w-3 shrink-0 bg-text-tertiary"
                />
              ) : null}
              <span
                className={cx(
                  "truncate",
                  row.tree ? "text-text-primary" : "text-text-secondary",
                )}
              >
                {row.name}
              </span>
            </span>
            {row.tree ? (
              <span className="ms-auto inline-flex shrink-0 items-center gap-2 rounded-md border border-border-button-default bg-background-primary-default px-2 py-0.5 text-caption-1-regular">
                <span className="text-text-primary">{row.tree.name}</span>
                <span className="text-text-tertiary">on</span>
                <span className="text-text-secondary">{row.tree.branch}</span>
              </span>
            ) : row.note ? (
              <span className="ms-auto shrink-0 text-caption-1-regular text-text-tertiary">
                {row.note}
              </span>
            ) : null}
          </li>
        ))}
      </ul>
    </DiagramFrame>
  );
}

const team = [
  {
    who: "alice",
    branch: "feature/auth",
    label: "",
    paths: "services/api/src/auth.rs",
    when: "2 min",
  },
  {
    who: "bob",
    branch: "main",
    label: "spike",
    paths: "apps/web/app/login.tsx",
    when: "9 min",
  },
  {
    who: "carol",
    branch: "feature/auth",
    label: "",
    paths: "services/api/src/auth.rs",
    when: "14 min",
  },
];

/** `wt status --team`: whose work in progress is where, with the overlap called out. */
export function TeamStatus() {
  return (
    <DiagramFrame
      label="wt status --team"
      live
      footer={withCode(
        "`wt push` prints the same overlap as a note and never blocks: staged snapshots are visible, not locks.",
      )}
      bodyClassName="p-0 sm:p-0"
    >
      <div className="overflow-x-auto">
        <table className="w-full min-w-[34rem] text-start font-mono text-[0.8125rem]">
          <thead>
            <tr className="border-b border-separator-border text-caption-1-regular text-text-tertiary">
              {["who", "branch", "label", "paths", "staged"].map((h) => (
                <th
                  key={h}
                  className="px-4 py-2.5 text-start font-normal sm:px-5"
                >
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {team.map((row) => {
              const overlap = row.paths === "services/api/src/auth.rs";
              return (
                <tr
                  key={row.who}
                  className="border-b border-separator-border last:border-b-0"
                >
                  <td className="px-4 py-2.5 text-text-primary sm:px-5">
                    {row.who}
                  </td>
                  <td className="px-4 py-2.5 text-text-secondary sm:px-5">
                    {row.branch}
                  </td>
                  <td className="px-4 py-2.5 text-text-secondary sm:px-5">
                    {row.label || "default"}
                  </td>
                  <td
                    className={cx(
                      "px-4 py-2.5 sm:px-5",
                      overlap
                        ? "hatch text-text-primary"
                        : "text-text-secondary",
                    )}
                  >
                    {row.paths}
                  </td>
                  <td className="px-4 py-2.5 whitespace-nowrap text-text-tertiary sm:px-5">
                    {row.when} ago
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <p className="border-t border-separator-border px-4 py-3 font-mono text-[0.8125rem] text-text-secondary sm:px-5">
        note: alice and carol have staged changes to services/api/src/auth.rs
      </p>
    </DiagramFrame>
  );
}

const services: [string, string, string][] = [
  [
    "gRPC sync",
    "worktree.sync.v1",
    "Push, fetch, clone, stage, packs with deltas",
  ],
  ["REST API", "/api/v1", "Every server operation, described by OpenAPI 3.1"],
  [
    "Live events",
    "/api/v1/events",
    "Server-sent events for pushes, reviews and admin changes",
  ],
  [
    "Admin panel",
    "/admin",
    "Trees, history, merge requests, IAM and operations",
  ],
];

/** Everything the server serves on its one port. */
export function ServerStack() {
  return (
    <DiagramFrame
      label="worktree-server --addr :7420"
      live
      footer={withCode(
        "Metadata lives in one embedded bbolt file and objects on disk or in S3-compatible storage: no external database to run.",
      )}
      bodyClassName="p-0 sm:p-0"
    >
      <ul className="grid gap-px bg-separator-border">
        {services.map(([name, path, what]) => (
          <li
            key={name}
            className="grid gap-1 bg-surface-raised px-4 py-3.5 sm:grid-cols-[9rem_10rem_1fr] sm:items-center sm:gap-4 sm:px-5"
          >
            <span className="text-body-medium text-text-primary">{name}</span>
            <span className="font-mono text-caption-1-regular text-text-secondary">
              {path}
            </span>
            <span className="text-caption-1-regular text-text-secondary">
              {what}
            </span>
          </li>
        ))}
      </ul>
    </DiagramFrame>
  );
}

export interface Comparison {
  topic: string;
  git: string;
  wt: string;
}

/** Git and W0rkTree side by side, one row per topic. */
export function CompareTable({ rows }: { rows: Comparison[] }) {
  return (
    <div className="overflow-hidden rounded-2xl border border-border-button-default">
      <div className="hidden grid-cols-[12rem_1fr_1fr] border-b border-separator-border bg-surface-sunken text-caption-1-semibold text-text-primary md:grid">
        <span className="px-5 py-3">Topic</span>
        <span className="border-s border-separator-border px-5 py-3 text-text-secondary">
          Git
        </span>
        <span className="border-s border-separator-border px-5 py-3">
          W0rkTree
        </span>
      </div>
      <ul className="grid gap-px bg-separator-border">
        {rows.map((row) => (
          <li
            key={row.topic}
            className="grid bg-background-full md:grid-cols-[12rem_1fr_1fr]"
          >
            <span className="px-5 pt-4 text-body-medium text-text-primary md:py-4">
              {row.topic}
            </span>
            <span className="px-5 py-2 text-body-regular leading-6 text-text-secondary md:border-s md:border-separator-border md:py-4">
              <span className="eyebrow me-2 md:hidden">Git</span>
              {withCode(row.git)}
            </span>
            <span className="px-5 pb-4 text-body-regular leading-6 text-text-primary md:border-s md:border-separator-border md:py-4">
              <span className="eyebrow me-2 md:hidden">W0rkTree</span>
              {withCode(row.wt)}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

export interface Question {
  q: string;
  a: string;
}

/** Native disclosure rows in a hairline list: keyboard and screen-reader friendly without script. */
export function FaqList({ items }: { items: Question[] }) {
  return (
    <ul className="grid gap-px overflow-hidden rounded-2xl border border-border-button-default bg-separator-border">
      {items.map((item) => (
        <li key={item.q} className="bg-background-full">
          <details className="group">
            <summary className="flex cursor-pointer list-none items-center gap-4 px-5 py-4 text-body-medium text-text-primary transition-colors hover:bg-background-secondary-default [&::-webkit-details-marker]:hidden">
              <span className="flex-1">{item.q}</span>
              <RiAddLine
                className="size-4 shrink-0 text-foreground-icon-secondary transition-transform duration-200 group-open:rotate-45"
                aria-hidden
              />
            </summary>
            <p className="px-5 pb-5 text-body-regular leading-6 text-pretty text-text-secondary">
              {withCode(item.a)}
            </p>
          </details>
        </li>
      ))}
    </ul>
  );
}
