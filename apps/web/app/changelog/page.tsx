import type { Metadata } from "next";
import { Badge } from "@/components/ui/badge";

export const metadata: Metadata = {
  title: "Changelog - W0rkTree",
  description: "Release history and updates for W0rkTree.",
};

interface Change {
  type: "Added" | "Changed" | "Fixed" | "Breaking";
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
    version: "v0.3.0-alpha",
    date: "June 2025",
    title: "Linked Branches & Atomic Merges",
    description:
      "This release introduces linked branch creation across nested trees, atomic multi-tree merge operations, and cross-tree conflict detection — laying the groundwork for truly unified multi-repo workflows.",
    changes: [
      {
        type: "Added",
        text: "Linked branch creation that spans multiple nested trees simultaneously.",
      },
      {
        type: "Added",
        text: "Atomic multi-tree merge ensuring all-or-nothing consistency across trees.",
      },
      {
        type: "Added",
        text: "Cross-tree conflict detection that surfaces merge issues before they propagate.",
      },
      {
        type: "Changed",
        text: "Merge algorithm now uses three-way merge for improved accuracy and fewer false conflicts.",
      },
      {
        type: "Fixed",
        text: "Branch switching race condition that could leave working directory in an inconsistent state.",
      },
    ],
  },
  {
    version: "v0.2.0-alpha",
    date: "April 2025",
    title: "Auto Tracking & Dependency Graph",
    description:
      "Automatic change tracking arrives with a background daemon, configurable snapshot rules, and a cross-tree dependency graph that keeps your monorepo in sync.",
    changes: [
      {
        type: "Added",
        text: "Background daemon for continuous filesystem monitoring and automatic snapshots.",
      },
      {
        type: "Added",
        text: "Auto-snapshot rules configurable per tree with debounce, glob filters, and thresholds.",
      },
      {
        type: "Added",
        text: "Cross-tree dependency declarations for modelling relationships between nested trees.",
      },
      {
        type: "Added",
        text: "Auto TODO generation that creates tracked tasks from dependency-change events.",
      },
      {
        type: "Changed",
        text: "Config format migrated from JSON to TOML for improved readability and comments support.",
      },
      {
        type: "Fixed",
        text: "Debounce timer accuracy on macOS that caused premature snapshot triggers.",
      },
    ],
  },
  {
    version: "v0.1.0-alpha",
    date: "February 2025",
    title: "Initial Alpha",
    description:
      "The first public alpha of W0rkTree — introducing the nested tree model, basic branching and snapshot creation, content-addressable storage powered by BLAKE3, and one-command Git import.",
    changes: [
      {
        type: "Added",
        text: "Nested tree model allowing hierarchical repository composition.",
      },
      {
        type: "Added",
        text: "Basic branching with create, switch, and list operations.",
      },
      {
        type: "Added",
        text: "Snapshot creation with full content-addressable storage backed by BLAKE3 hashing.",
      },
      {
        type: "Added",
        text: "Git import command to convert existing Git repositories into W0rkTree trees.",
      },
    ],
  },
];

function changeBadgeVariant(
  type: "Added" | "Changed" | "Fixed" | "Breaking",
): "secondary" | "outline" | "default" | "destructive" {
  switch (type) {
    case "Added":
      return "secondary";
    case "Changed":
      return "outline";
    case "Fixed":
      return "default";
    case "Breaking":
      return "destructive";
  }
}

export default function ChangelogPage() {
  return (
    <div className="container mx-auto max-w-4xl px-6 py-16 md:py-24">
      {/* Hero */}
      <section className="mb-16 text-center">
        <h1 className="text-4xl font-bold tracking-tight sm:text-5xl">
          Changelog
        </h1>
        <p className="mt-4 text-lg text-muted-foreground">
          Every release, every improvement — tracked in one place.
        </p>
      </section>

      {/* Releases timeline */}
      <section className="relative space-y-12">
        {/* Vertical timeline line */}
        <div className="absolute left-[7px] top-2 bottom-0 w-px bg-border" />

        {releases.map((release) => (
          <article key={release.version} className="relative pl-10">
            {/* Timeline dot */}
            <div className="absolute left-0 top-2 h-[15px] w-[15px] rounded-full border-2 border-primary bg-background" />

            {/* Version & date header */}
            <div className="flex flex-wrap items-center gap-3">
              <Badge>{release.version}</Badge>
              <span className="text-sm text-muted-foreground">
                {release.date}
              </span>
            </div>

            {/* Title */}
            <h2 className="mt-3 text-2xl font-semibold tracking-tight">
              {release.title}
            </h2>

            {/* Description */}
            <p className="mt-2 text-muted-foreground leading-relaxed">
              {release.description}
            </p>

            {/* Changes list */}
            <ul className="mt-5 space-y-3">
              {release.changes.map((change, idx) => (
                <li key={idx} className="flex items-start gap-3">
                  <Badge
                    variant={changeBadgeVariant(change.type)}
                    className="mt-0.5 shrink-0 text-[11px] min-w-[68px] justify-center"
                  >
                    {change.type}
                  </Badge>
                  <span className="text-sm text-foreground/90 leading-relaxed">
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
