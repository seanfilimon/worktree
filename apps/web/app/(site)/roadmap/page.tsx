import type { Metadata } from "next";
import Link from "next/link";
import {
  RiArrowRightLine,
  RiArrowRightUpLine,
  RiCheckLine,
  RiHistoryLine,
  RiTimeLine,
} from "react-icons/ri";
import { ActionLink, TextLink } from "@/components/site/action";
import { Band, SectionHeading, gutter } from "@/components/site/frame";
import { StaggerChild, StaggerList } from "@/components/site/motion-text";
import { PageHero } from "@/components/site/page-hero";
import { HoverHatch, RuleItem, hairlineGrid } from "@/components/features/cells";
import {
  type Milestone,
  milestones,
  plannedMilestones,
  shippedMilestones,
} from "@/components/features/roadmap-data";
import { ISSUES_URL } from "@/lib/site";
import { cx } from "@/utils/cx";

export const metadata: Metadata = {
  title: "Roadmap",
  description: "What shipped in W0rkTree up to v0.8 and what is planned next.",
};

const inlineLink =
  "text-text-primary underline decoration-text-tertiary underline-offset-4 transition-colors hover:decoration-text-primary";

function slug(text: string) {
  return text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
}

function milestoneId(milestone: Milestone) {
  return `m-${slug(milestone.version ?? milestone.title)}`;
}

/** The changelog entry a shipped milestone corresponds to ("v0.3–v0.4" is the release "v0.3.0–v0.4.0"). */
function releaseNotesHref(version: string) {
  const release = version
    .split("–")
    .map((part) => `${part}.0`)
    .join("–");
  return `/changelog#${slug(release)}`;
}

function capitalize(text: string) {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

export default function RoadmapPage() {
  const latest = shippedMilestones[shippedMilestones.length - 1];
  return (
    <>
      <PageHero
        eyebrow="Roadmap"
        title="What shipped,"
        muted="and what is planned next."
        actions={
          <>
            <ActionLink href="/changelog" leadingIcon={RiHistoryLine}>
              Changelog
            </ActionLink>
            <ActionLink
              href={ISSUES_URL}
              variant="secondary"
              trailingIcon={RiArrowRightUpLine}
              target="_blank"
              rel="noopener noreferrer"
            >
              Issue tracker
            </ActionLink>
          </>
        }
        meta={`${shippedMilestones.length} milestones shipped${
          latest?.version ? `, up to ${latest.version}` : ""
        } · ${plannedMilestones.length} planned`}
      >
        Everything under &ldquo;Planned&rdquo; is not available yet. Priorities can change;
        discussion happens on the{" "}
        <a href={ISSUES_URL} target="_blank" rel="noopener noreferrer" className={inlineLink}>
          issue tracker
        </a>
        . See the{" "}
        <Link href="/changelog" className={inlineLink}>
          changelog
        </Link>{" "}
        for released work.
      </PageHero>

      <Overview />
      <ShippedSection />
      <PlannedSection />

      <Band marks={false}>
        <div
          className={cx(
            "relative flex flex-col items-start justify-between gap-6 overflow-hidden py-12 md:flex-row md:items-center",
            gutter,
          )}
        >
          <div aria-hidden className="hatch pointer-events-none absolute inset-0 opacity-50" />
          <div className="relative flex flex-col gap-2">
            <p className="eyebrow">Discussion</p>
            <p className="text-title-2-semibold tracking-[-0.02em]">
              Priorities are discussed on the issue tracker.
            </p>
          </div>
          <div className="relative flex flex-wrap gap-2.5">
            <ActionLink
              href={ISSUES_URL}
              trailingIcon={RiArrowRightUpLine}
              target="_blank"
              rel="noopener noreferrer"
            >
              Issue tracker
            </ActionLink>
            <ActionLink href="/changelog" variant="secondary" leadingIcon={RiHistoryLine}>
              Changelog
            </ActionLink>
          </div>
        </div>
      </Band>
    </>
  );
}

/** Every milestone in order, as one strip: solid cells shipped, hatched cells planned. */
function Overview() {
  return (
    <Band>
      <nav aria-label="Milestones">
        <StaggerList className={cx(hairlineGrid, "grid-cols-2 sm:grid-cols-5")}>
          {milestones.map((milestone) => {
            const shipped = milestone.status === "shipped";
            return (
              <li key={milestoneId(milestone)} className="bg-background-full">
                <StaggerChild className="h-full">
                  <a
                    href={`#${milestoneId(milestone)}`}
                    className="group relative flex h-full flex-col gap-3 overflow-hidden p-4 transition-colors duration-200 hover:bg-background-secondary-default sm:p-5"
                  >
                    {shipped ? (
                      <HoverHatch />
                    ) : (
                      <span
                        aria-hidden
                        className="hatch pointer-events-none absolute inset-0 opacity-70"
                      />
                    )}
                    <span className="relative flex items-center gap-2">
                      <StatusMark shipped={shipped} />
                      <span className="eyebrow">{shipped ? "Shipped" : "Planned"}</span>
                    </span>
                    <span className="relative text-body-2-medium text-pretty">
                      {milestone.version ? (
                        <span className="font-mono tabular-nums">{milestone.version}</span>
                      ) : (
                        milestone.title
                      )}
                    </span>
                  </a>
                </StaggerChild>
              </li>
            );
          })}
        </StaggerList>
      </nav>
    </Band>
  );
}

function StatusMark({ shipped }: { shipped: boolean }) {
  return shipped ? (
    <span className="flex size-4 shrink-0 items-center justify-center rounded-full bg-text-primary text-background-full">
      <RiCheckLine className="size-3" aria-hidden />
    </span>
  ) : (
    <span
      aria-hidden
      className="size-4 shrink-0 rounded-full border border-dashed border-text-tertiary bg-background-full"
    />
  );
}

function ShippedSection() {
  return (
    <Band id="shipped">
      <div className={cx("py-16 md:py-20", gutter)}>
        <SectionHeading
          index="01"
          eyebrow="Shipped"
          title={`Released, up to ${shippedMilestones[shippedMilestones.length - 1]?.version ?? ""}`}
        >
          Each milestone links to its release notes in the changelog.
        </SectionHeading>
      </div>
      <ol className={cx(hairlineGrid, "border-t border-separator-border")}>
        {shippedMilestones.map((milestone) => {
          const id = milestoneId(milestone);
          return (
            <li
              key={id}
              id={id}
              className="scroll-mt-28 bg-background-full"
            >
              <div
                className={cx(
                  "grid gap-6 py-10 md:py-12 lg:grid-cols-12 lg:gap-10",
                  gutter,
                )}
              >
                <div className="flex flex-col gap-3 lg:col-span-3">
                  <p className="flex items-center gap-2.5">
                    <StatusMark shipped />
                    <span className="font-mono text-title-2-medium tracking-[-0.03em] tabular-nums">
                      {milestone.version}
                    </span>
                  </p>
                  {milestone.version ? (
                    <TextLink
                      href={releaseNotesHref(milestone.version)}
                      trailingIcon={RiArrowRightLine}
                      className="text-body-2-medium text-text-secondary hover:text-text-primary"
                    >
                      Release notes
                    </TextLink>
                  ) : null}
                </div>
                <div className="flex flex-col gap-2 lg:col-span-4">
                  <h3 id={`${id}-title`} className="text-title-3-semibold text-balance">
                    {capitalize(milestone.title)}
                  </h3>
                  <p className="text-body-regular leading-6 text-pretty text-text-secondary">
                    {milestone.summary}
                  </p>
                </div>
                <ul className="flex flex-col gap-2 lg:col-span-5">
                  {milestone.items.map((item) => (
                    <RuleItem key={item}>{item}</RuleItem>
                  ))}
                </ul>
              </div>
            </li>
          );
        })}
      </ol>
    </Band>
  );
}

function PlannedSection() {
  return (
    <Band id="planned" frameClassName="bg-surface-sunken">
      <div className={cx("py-16 md:py-20", gutter)}>
        <SectionHeading index="02" eyebrow="Planned" title="Not available yet">
          Priorities can change; discussion happens on the issue tracker.
        </SectionHeading>
      </div>
      <StaggerList
        className={cx(hairlineGrid, "border-t border-separator-border sm:grid-cols-2")}
      >
        {plannedMilestones.map((milestone) => {
          const id = milestoneId(milestone);
          return (
            <li key={id} id={id} className="scroll-mt-28 bg-surface-sunken">
              <StaggerChild className="relative flex h-full flex-col gap-5 overflow-hidden p-6 sm:p-8">
                <span
                  aria-hidden
                  className="hatch pointer-events-none absolute inset-0 opacity-60"
                />
                <span className="relative flex items-center gap-2">
                  <RiTimeLine className="size-4 text-foreground-icon-tertiary" aria-hidden />
                  <span className="eyebrow">Planned</span>
                </span>
                <div className="relative flex flex-col gap-2">
                  <h3 className="text-title-3-semibold">{milestone.title}</h3>
                  <p className="text-body-regular leading-6 text-pretty text-text-secondary">
                    {milestone.summary}
                  </p>
                </div>
                <ul className="relative flex flex-col gap-2">
                  {milestone.items.map((item) => (
                    <RuleItem key={item}>{item}</RuleItem>
                  ))}
                </ul>
              </StaggerChild>
            </li>
          );
        })}
      </StaggerList>
    </Band>
  );
}
