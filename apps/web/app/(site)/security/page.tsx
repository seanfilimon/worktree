import type { Metadata } from "next";
import Link from "next/link";
import {
  RiArrowRightLine,
  RiArrowRightUpLine,
  RiFileList3Line,
  RiFingerprintLine,
  RiGitBranchLine,
  RiKey2Line,
  RiLockLine,
  RiQuillPenLine,
  RiRadarLine,
  RiShieldUserLine,
  RiSwapLine,
} from "react-icons/ri";
import { IconTile, StepNumber, type Icon } from "@/components/project/cells";
import { PolicyNav } from "@/components/project/prose-aside";
import { ActionLink } from "@/components/site/action";
import { Band, SectionHeading, gutter } from "@/components/site/frame";
import { StaggerChild, StaggerList } from "@/components/site/motion-text";
import { PageHero, ProseBand } from "@/components/site/page-hero";
import { CURRENT_VERSION, REPO_URL } from "@/lib/site";
import { cx } from "@/utils/cx";

export const metadata: Metadata = {
  title: "Security",
  description:
    "How W0rkTree protects your code and credentials, and how to report a vulnerability.",
};

interface Control {
  icon: Icon;
  title: string;
  description: string;
  roadmap?: boolean;
}

const controls: Control[] = [
  {
    icon: RiRadarLine,
    title: "Secret Scanning",
    description:
      "Added and modified files are scanned before every snapshot. Findings block the snapshot and are reported with redacted excerpts.",
  },
  {
    icon: RiGitBranchLine,
    title: "Append-Only History",
    description:
      "Snapshots are immutable. Reverts add new snapshots, branches are archived instead of deleted, and the server has no force push.",
  },
  {
    icon: RiFingerprintLine,
    title: "Password Hashing",
    description: "Account passwords are hashed with argon2id.",
  },
  {
    icon: RiKey2Line,
    title: "Tokens",
    description:
      "Bearer tokens on every gRPC and REST call. Session tokens expire; personal API tokens can carry a TTL and are stored only as SHA-256 hashes.",
  },
  {
    icon: RiShieldUserLine,
    title: "Authorization",
    description:
      "Five tenant roles, private/internal/public tree visibility, branch protection and per-path license rules, all enforced by the server.",
  },
  {
    icon: RiFileList3Line,
    title: "Audit Log",
    description:
      "Pushes, sign-ins and administrative changes are written to an append-only audit log, visible to tenant and server admins.",
  },
  {
    icon: RiSwapLine,
    title: "Transport",
    description:
      "The server speaks cleartext HTTP/2 (h2c). Run it behind a TLS-terminating reverse proxy for anything beyond localhost.",
  },
  {
    icon: RiQuillPenLine,
    title: "Snapshot Signing",
    description: "Cryptographic signatures on snapshots.",
    roadmap: true,
  },
  {
    icon: RiLockLine,
    title: "Encryption at Rest",
    description: "Server-side encryption of metadata and objects.",
    roadmap: true,
  },
];

const advisoryUrl = `${REPO_URL}/security/advisories/new`;

export default function SecurityPage() {
  return (
    <>
      <PageHero
        eyebrow="Security"
        title="Security."
        muted="How W0rkTree protects your code and credentials."
        actions={
          <>
            <ActionLink href="/docs/security" trailingIcon={RiArrowRightLine}>
              Security documentation
            </ActionLink>
            <ActionLink
              href={advisoryUrl}
              variant="secondary"
              trailingIcon={RiArrowRightUpLine}
              target="_blank"
              rel="noopener noreferrer"
            >
              Report a vulnerability
            </ActionLink>
          </>
        }
      >
        The controls below describe W0rkTree {CURRENT_VERSION}. For deployment guidance and the full
        threat model, read the{" "}
        <Link
          href="/docs/security"
          className="text-text-primary underline decoration-text-tertiary underline-offset-4 transition-colors hover:decoration-current"
        >
          security documentation
        </Link>
        .
      </PageHero>

      <Band id="controls">
        <div className={cx("py-14 md:py-16", gutter)}>
          <SectionHeading index="01" eyebrow="Controls" title={`Controls in ${CURRENT_VERSION}`} />
        </div>
        <StaggerList className="grid gap-px border-t border-separator-border bg-separator-border md:grid-cols-2 lg:grid-cols-3">
          {controls.map(({ icon, ...control }, index) => (
            <li key={control.title} className="bg-background-full">
              {/* Roadmap items are hatched: planned, not yet shipped. */}
              <StaggerChild
                className={cx(
                  "relative flex h-full flex-col gap-2 p-6 sm:p-8",
                  control.roadmap && "hatch",
                )}
              >
                <span className="mb-3 flex items-center justify-between gap-3">
                  <IconTile icon={icon} className={cx(control.roadmap && "border-dashed shadow-none")} />
                  {control.roadmap ? (
                    <span className="rounded-md border border-dashed border-border-button-default bg-background-full px-1.5 py-0.5 font-mono text-caption-1-regular text-text-secondary">
                      Roadmap
                    </span>
                  ) : (
                    <StepNumber value={index + 1} />
                  )}
                </span>
                <h3 className={cx("text-headline-medium", control.roadmap && "text-text-secondary")}>
                  {control.title}
                </h3>
                <p className="text-body-regular leading-6 text-pretty text-text-secondary">
                  {control.description}
                </p>
              </StaggerChild>
            </li>
          ))}
        </StaggerList>
      </Band>

      <ProseBand className="[&_h2]:scroll-mt-28" aside={<PolicyNav current="/security" />}>
        <p className="eyebrow not-prose">02 · Disclosure</p>
        <h2 id="report">Reporting a vulnerability</h2>
        <p>
          Please do not open a public issue for security problems. Report them privately through{" "}
          <a href={advisoryUrl} target="_blank" rel="noopener noreferrer">
            GitHub security advisories
          </a>{" "}
          with steps to reproduce, the affected component (CLI, daemon, server or admin panel) and the
          commit you tested.
        </p>
        <div className="not-prose mt-8">
          <ActionLink
            href={advisoryUrl}
            trailingIcon={RiArrowRightUpLine}
            target="_blank"
            rel="noopener noreferrer"
          >
            Open a private advisory
          </ActionLink>
        </div>
      </ProseBand>
    </>
  );
}
