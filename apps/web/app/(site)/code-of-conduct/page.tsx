import type { Metadata } from "next";
import Link from "next/link";
import { RiArrowRightLine, RiMailLine } from "react-icons/ri";
import { StepNumber } from "@/components/project/cells";
import { AnchorAction } from "@/components/project/mail-action";
import { OnThisPage, PolicyNav } from "@/components/project/prose-aside";
import { ActionLink } from "@/components/site/action";
import { PageHero, ProseBand } from "@/components/site/page-hero";
import { DISCORD_URL } from "@/lib/site";

export const metadata: Metadata = {
  title: "Code of Conduct",
  description: "W0rkTree community code of conduct.",
};

const sections = [
  { id: "pledge", label: "Our Pledge" },
  { id: "standards", label: "Our Standards" },
  { id: "responsibilities", label: "Enforcement Responsibilities" },
  { id: "scope", label: "Scope" },
  { id: "enforcement", label: "Enforcement" },
  { id: "attribution", label: "Attribution" },
];

const guidelines = [
  {
    title: "Correction",
    impact: "Use of inappropriate language or other behavior deemed unprofessional or unwelcome.",
    consequence:
      "A private, written warning from community leaders, providing clarity around the nature of the violation and an explanation of why the behavior was inappropriate. A public apology may be requested.",
  },
  {
    title: "Warning",
    impact: "A violation through a single incident or series of actions.",
    consequence:
      "A warning with consequences for continued behavior. No interaction with the people involved, including unsolicited interaction with those enforcing the Code of Conduct, for a specified period of time. Violating these terms may lead to a temporary or permanent ban.",
  },
  {
    title: "Temporary Ban",
    impact:
      "A serious violation of community standards, including sustained inappropriate behavior.",
    consequence:
      "A temporary ban from any sort of interaction or public communication with the community for a specified period of time. Violating these terms may lead to a permanent ban.",
  },
  {
    title: "Permanent Ban",
    impact:
      "Demonstrating a pattern of violation of community standards, including sustained inappropriate behavior, harassment of an individual, or aggression toward or disparagement of classes of individuals.",
    consequence: "A permanent ban from any sort of public interaction within the community.",
  },
];

export default function CodeOfConductPage() {
  return (
    <>
      <PageHero
        eyebrow="Community"
        title="Code of Conduct"
        actions={
          <>
            <AnchorAction href="mailto:conduct@worktree.dev" variant="primary" leadingIcon={RiMailLine}>
              conduct@worktree.dev
            </AnchorAction>
            <ActionLink href="/contributing" variant="secondary" trailingIcon={RiArrowRightLine}>
              Contributing
            </ActionLink>
          </>
        }
        meta="Adapted from the Contributor Covenant, version 2.1."
      >
        A guide to community expectations and behavior for all W0rkTree contributors and
        participants.
      </PageHero>

      <ProseBand
        className="[&_h2]:scroll-mt-28"
        aside={
          <>
            <OnThisPage items={sections} />
            <PolicyNav current="/code-of-conduct" />
          </>
        }
      >
        <h2 id="pledge">Our Pledge</h2>
        <p>
          We as members, contributors, and leaders pledge to make participation in the W0rkTree
          community a harassment-free experience for everyone, regardless of age, body size, visible
          or invisible disability, ethnicity, sex characteristics, gender identity and expression,
          level of experience, education, socio-economic status, nationality, personal appearance,
          race, caste, color, religion, or sexual identity and orientation.
        </p>
        <p>
          We pledge to act and interact in ways that contribute to an open, welcoming, diverse,
          inclusive, and healthy community.
        </p>

        <h2 id="standards">Our Standards</h2>
        <p>
          Examples of behavior that contributes to a positive environment for our community include:
        </p>
        <ul>
          <li>Using welcoming and inclusive language</li>
          <li>Being respectful of differing viewpoints and experiences</li>
          <li>Gracefully accepting constructive criticism</li>
          <li>Focusing on what is best for the community</li>
          <li>Showing empathy towards other community members</li>
        </ul>
        <p>Examples of unacceptable behavior include:</p>
        <ul>
          <li>
            The use of sexualized language or imagery, and sexual attention or advances of any kind
          </li>
          <li>Trolling, insulting or derogatory comments, and personal or political attacks</li>
          <li>Public or private harassment</li>
          <li>
            Publishing others&apos; private information, such as a physical or email address, without
            their explicit permission
          </li>
          <li>
            Other conduct which could reasonably be considered inappropriate in a professional setting
          </li>
        </ul>

        <h2 id="responsibilities">Enforcement Responsibilities</h2>
        <p>
          Community leaders are responsible for clarifying and enforcing our standards of acceptable
          behavior and will take appropriate and fair corrective action in response to any behavior
          that they deem inappropriate, threatening, offensive, or harmful.
        </p>
        <p>
          Community leaders have the right and responsibility to remove, edit, or reject comments,
          commits, code, wiki edits, issues, and other contributions that are not aligned to this Code
          of Conduct, and will communicate reasons for moderation decisions when appropriate.
        </p>

        <h2 id="scope">Scope</h2>
        <p>
          This Code of Conduct applies within all community spaces, and also applies when an
          individual is officially representing the community in public spaces. Examples of
          representing our community include using an official e-mail address, posting via an
          official social media account, or acting as an appointed representative at an online or
          offline event.
        </p>

        <h2 id="enforcement">Enforcement</h2>
        <p>
          Instances of abusive, harassing, or otherwise unacceptable behavior may be reported to the
          community leaders responsible for enforcement at{" "}
          <a href="mailto:conduct@worktree.dev">conduct@worktree.dev</a>.
        </p>
        <p>
          All complaints will be reviewed and investigated promptly and fairly. All community leaders
          are obligated to respect the privacy and security of the reporter of any incident.
        </p>
        <p>
          Community leaders will follow these impact guidelines in determining the consequences for
          any action they deem in violation of this Code of Conduct:
        </p>
        {/* The four steps as a ladder: each one a hairline row, its number and name beside the detail. */}
        <ol className="not-prose my-8 grid gap-px overflow-hidden rounded-2xl border border-border-button-default bg-separator-border shadow-sm">
          {guidelines.map((step, index) => (
            <li
              key={step.title}
              className="grid gap-4 bg-surface-raised p-5 sm:grid-cols-[11rem_minmax(0,1fr)] sm:p-6"
            >
              <div className="flex items-baseline gap-3 sm:flex-col sm:gap-1.5">
                <StepNumber value={index + 1} />
                <h3 className="text-headline-medium">{step.title}</h3>
              </div>
              <dl className="flex flex-col gap-3 text-body-regular leading-6 text-text-secondary">
                <div>
                  <dt className="text-body-medium text-text-primary">Community Impact</dt>
                  <dd>{step.impact}</dd>
                </div>
                <div>
                  <dt className="text-body-medium text-text-primary">Consequence</dt>
                  <dd>{step.consequence}</dd>
                </div>
              </dl>
            </li>
          ))}
        </ol>

        <h2 id="attribution">Attribution</h2>
        <p>
          This Code of Conduct is adapted from the{" "}
          <a
            href="https://www.contributor-covenant.org/version/2/1/code_of_conduct/"
            target="_blank"
            rel="noopener noreferrer"
          >
            Contributor Covenant, version 2.1
          </a>
          .
        </p>
        <p>
          For answers to common questions about this code of conduct, see the{" "}
          <a href="https://www.contributor-covenant.org/faq" target="_blank" rel="noopener noreferrer">
            FAQ
          </a>
          . For questions specific to W0rkTree, reach out on{" "}
          <a href={DISCORD_URL} target="_blank" rel="noopener noreferrer">
            Discord
          </a>{" "}
          or visit the <Link href="/contributing">Contributing</Link> page.
        </p>
      </ProseBand>
    </>
  );
}
