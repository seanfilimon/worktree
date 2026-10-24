import Link from "next/link";
import type { Metadata } from "next";

export const metadata: Metadata = {
  title: "Code of Conduct - W0rkTree",
  description: "W0rkTree community code of conduct.",
};

export default function CodeOfConductPage() {
  return (
    <div className="container mx-auto max-w-3xl px-6 py-16">
      {/* Hero */}
      <section className="mb-16 text-center">
        <h1 className="mb-4 text-4xl font-bold tracking-tight sm:text-5xl">
          Code of Conduct
        </h1>
        <p className="mx-auto max-w-2xl text-lg text-muted-foreground">
          A guide to community expectations and behavior for all W0rkTree
          contributors and participants.
        </p>
      </section>

      {/* Our Pledge */}
      <section className="mb-12">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">
          Our Pledge
        </h2>
        <p className="mb-4 leading-relaxed text-muted-foreground">
          We as members, contributors, and leaders pledge to make participation
          in the W0rkTree community a harassment-free experience for everyone,
          regardless of age, body size, visible or invisible disability,
          ethnicity, sex characteristics, gender identity and expression, level
          of experience, education, socio-economic status, nationality, personal
          appearance, race, caste, color, religion, or sexual identity and
          orientation.
        </p>
        <p className="leading-relaxed text-muted-foreground">
          We pledge to act and interact in ways that contribute to an open,
          welcoming, diverse, inclusive, and healthy community.
        </p>
      </section>

      {/* Our Standards */}
      <section className="mb-12">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">
          Our Standards
        </h2>
        <p className="mb-4 leading-relaxed text-muted-foreground">
          Examples of behavior that contributes to a positive environment for our
          community include:
        </p>
        <ul className="mb-6 list-inside list-disc space-y-2 text-muted-foreground">
          <li>Using welcoming and inclusive language</li>
          <li>Being respectful of differing viewpoints and experiences</li>
          <li>Gracefully accepting constructive criticism</li>
          <li>Focusing on what is best for the community</li>
          <li>Showing empathy towards other community members</li>
        </ul>
        <p className="mb-4 leading-relaxed text-muted-foreground">
          Examples of unacceptable behavior include:
        </p>
        <ul className="mb-4 list-inside list-disc space-y-2 text-muted-foreground">
          <li>
            The use of sexualized language or imagery, and sexual attention or
            advances of any kind
          </li>
          <li>
            Trolling, insulting or derogatory comments, and personal or political
            attacks
          </li>
          <li>Public or private harassment</li>
          <li>
            Publishing others&apos; private information, such as a physical or
            email address, without their explicit permission
          </li>
          <li>
            Other conduct which could reasonably be considered inappropriate in a
            professional setting
          </li>
        </ul>
      </section>

      {/* Enforcement Responsibilities */}
      <section className="mb-12">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">
          Enforcement Responsibilities
        </h2>
        <p className="mb-4 leading-relaxed text-muted-foreground">
          Community leaders are responsible for clarifying and enforcing our
          standards of acceptable behavior and will take appropriate and fair
          corrective action in response to any behavior that they deem
          inappropriate, threatening, offensive, or harmful.
        </p>
        <p className="leading-relaxed text-muted-foreground">
          Community leaders have the right and responsibility to remove, edit, or
          reject comments, commits, code, wiki edits, issues, and other
          contributions that are not aligned to this Code of Conduct, and will
          communicate reasons for moderation decisions when appropriate.
        </p>
      </section>

      {/* Scope */}
      <section className="mb-12">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">Scope</h2>
        <p className="leading-relaxed text-muted-foreground">
          This Code of Conduct applies within all community spaces, and also
          applies when an individual is officially representing the community in
          public spaces. Examples of representing our community include using an
          official e-mail address, posting via an official social media account,
          or acting as an appointed representative at an online or offline event.
        </p>
      </section>

      {/* Enforcement */}
      <section className="mb-12">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">
          Enforcement
        </h2>
        <p className="mb-4 leading-relaxed text-muted-foreground">
          Instances of abusive, harassing, or otherwise unacceptable behavior
          may be reported to the community leaders responsible for enforcement
          at{" "}
          <a
            href="mailto:conduct@worktree.dev"
            className="font-medium text-foreground underline underline-offset-4 hover:text-primary"
          >
            conduct@worktree.dev
          </a>
          .
        </p>
        <p className="mb-4 leading-relaxed text-muted-foreground">
          All complaints will be reviewed and investigated promptly and fairly.
          All community leaders are obligated to respect the privacy and security
          of the reporter of any incident.
        </p>
        <p className="leading-relaxed text-muted-foreground">
          Community leaders will follow these impact guidelines in determining
          the consequences for any action they deem in violation of this Code of
          Conduct:
        </p>
        <div className="mt-6 space-y-4">
          <div className="rounded-lg border p-4">
            <h3 className="mb-1 font-medium">1. Correction</h3>
            <p className="text-sm text-muted-foreground">
              <strong className="text-foreground">Community Impact:</strong> Use
              of inappropriate language or other behavior deemed unprofessional
              or unwelcome.
            </p>
            <p className="mt-1 text-sm text-muted-foreground">
              <strong className="text-foreground">Consequence:</strong> A
              private, written warning from community leaders, providing clarity
              around the nature of the violation and an explanation of why the
              behavior was inappropriate. A public apology may be requested.
            </p>
          </div>
          <div className="rounded-lg border p-4">
            <h3 className="mb-1 font-medium">2. Warning</h3>
            <p className="text-sm text-muted-foreground">
              <strong className="text-foreground">Community Impact:</strong> A
              violation through a single incident or series of actions.
            </p>
            <p className="mt-1 text-sm text-muted-foreground">
              <strong className="text-foreground">Consequence:</strong> A warning
              with consequences for continued behavior. No interaction with the
              people involved, including unsolicited interaction with those
              enforcing the Code of Conduct, for a specified period of time.
              Violating these terms may lead to a temporary or permanent ban.
            </p>
          </div>
          <div className="rounded-lg border p-4">
            <h3 className="mb-1 font-medium">3. Temporary Ban</h3>
            <p className="text-sm text-muted-foreground">
              <strong className="text-foreground">Community Impact:</strong> A
              serious violation of community standards, including sustained
              inappropriate behavior.
            </p>
            <p className="mt-1 text-sm text-muted-foreground">
              <strong className="text-foreground">Consequence:</strong> A
              temporary ban from any sort of interaction or public communication
              with the community for a specified period of time. Violating these
              terms may lead to a permanent ban.
            </p>
          </div>
          <div className="rounded-lg border p-4">
            <h3 className="mb-1 font-medium">4. Permanent Ban</h3>
            <p className="text-sm text-muted-foreground">
              <strong className="text-foreground">Community Impact:</strong>{" "}
              Demonstrating a pattern of violation of community standards,
              including sustained inappropriate behavior, harassment of an
              individual, or aggression toward or disparagement of classes of
              individuals.
            </p>
            <p className="mt-1 text-sm text-muted-foreground">
              <strong className="text-foreground">Consequence:</strong> A
              permanent ban from any sort of public interaction within the
              community.
            </p>
          </div>
        </div>
      </section>

      {/* Attribution */}
      <section className="mb-8">
        <h2 className="mb-4 text-2xl font-semibold tracking-tight">
          Attribution
        </h2>
        <p className="mb-4 leading-relaxed text-muted-foreground">
          This Code of Conduct is adapted from the{" "}
          <a
            href="https://www.contributor-covenant.org/version/2/1/code_of_conduct/"
            target="_blank"
            rel="noopener noreferrer"
            className="font-medium text-foreground underline underline-offset-4 hover:text-primary"
          >
            Contributor Covenant, version 2.1
          </a>
          .
        </p>
        <p className="leading-relaxed text-muted-foreground">
          For answers to common questions about this code of conduct, see the{" "}
          <a
            href="https://www.contributor-covenant.org/faq"
            target="_blank"
            rel="noopener noreferrer"
            className="font-medium text-foreground underline underline-offset-4 hover:text-primary"
          >
            FAQ
          </a>
          . For questions specific to W0rkTree, reach out on{" "}
          <a
            href="https://discord.gg/worktree"
            target="_blank"
            rel="noopener noreferrer"
            className="font-medium text-foreground underline underline-offset-4 hover:text-primary"
          >
            Discord
          </a>{" "}
          or visit the{" "}
          <Link
            href="/contributing"
            className="font-medium text-foreground underline underline-offset-4 hover:text-primary"
          >
            Contributing
          </Link>{" "}
          page.
        </p>
      </section>
    </div>
  );
}
