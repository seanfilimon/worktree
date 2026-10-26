import type { Metadata } from "next";
import { RiArrowRightLine, RiMap2Line, RiStackLine } from "react-icons/ri";
import { Points } from "@/components/home/section";
import { LinkGrid } from "@/components/project/cells";
import { ActionLink } from "@/components/site/action";
import { CreatorCredit } from "@/components/site/creator-credit";
import { Band, SectionHeading, gutter } from "@/components/site/frame";
import { PageHero } from "@/components/site/page-hero";
import { cx } from "@/utils/cx";

export const metadata: Metadata = {
  title: "About",
  description: "Why W0rkTree exists and how it is built.",
};

const principles = [
  {
    title: "Client and server, each in its best language",
    text: "The wt CLI, SDK and background daemon are Rust; the multi-tenant server is a single Go binary that serves gRPC sync, REST, live events and the admin panel on one port.",
  },
  {
    title: "Staged visibility",
    text: "Work in progress can be shared with the team as staged snapshots, without touching branch history.",
  },
  {
    title: "Multi-tenancy built in",
    text: "Accounts, organizations, roles and tree visibility are part of the server, not an add-on.",
  },
  {
    title: "Governance at the server",
    text: "Branch protection, merge requests and per-path SPDX license rules are enforced on every push.",
  },
  {
    title: "Nothing is rewritten",
    text: "History is append-only: reverts add snapshots, branches are archived, and there is no force push.",
  },
];

export default function AboutPage() {
  return (
    <>
      <PageHero
        eyebrow="About"
        title="About W0rkTree."
        muted="Why it exists and how it is built."
        actions={
          <>
            <ActionLink href="/guides/architecture" trailingIcon={RiArrowRightLine}>
              Architecture overview
            </ActionLink>
            <ActionLink href="/roadmap" variant="secondary">
              Roadmap
            </ActionLink>
          </>
        }
      >
        <p>
          W0rkTree is not a Git wrapper. It is a version control system with its own object model,
          sync protocol and server, built to address the collaboration and access-control gaps teams
          work around in Git today. Git import and export make it possible to move over gradually.
        </p>
        <CreatorCredit className="mt-6" />
      </PageHero>

      <Band id="philosophy" frameClassName="bg-surface-sunken">
        <div className={cx("grid gap-10 py-16 md:py-20 lg:grid-cols-12", gutter)}>
          <SectionHeading
            index="01"
            eyebrow="Philosophy"
            title="Technical Philosophy"
            className="lg:col-span-5"
          />
          <Points
            className="lg:col-span-6 lg:col-start-7 lg:pt-10"
            points={principles.map((principle) => ({
              lead: `${principle.title}.`,
              body: principle.text,
            }))}
          />
        </div>
      </Band>

      <Band id="next">
        <div className={cx("py-14 md:py-16", gutter)}>
          <SectionHeading index="02" eyebrow="Read on" title="Go deeper" />
        </div>
        <LinkGrid
          columns={2}
          items={[
            {
              icon: RiStackLine,
              title: "Architecture overview",
              body: "Read the architecture overview.",
              href: "/guides/architecture",
            },
            {
              icon: RiMap2Line,
              title: "Roadmap",
              body: "See what is planned on the roadmap.",
              href: "/roadmap",
            },
          ]}
        />
      </Band>
    </>
  );
}
