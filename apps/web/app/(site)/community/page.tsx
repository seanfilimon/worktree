import type { Metadata } from "next";
import {
  RiArrowRightLine,
  RiArrowRightUpLine,
  RiArticleLine,
  RiBookOpenLine,
  RiDiscordLine,
  RiGithubLine,
  RiHashtag,
  RiHeartLine,
  RiTeamLine,
} from "react-icons/ri";
import { DiagramFrame } from "@/components/home/frame";
import { Reveal } from "@/components/home/motion";
import { LinkGrid } from "@/components/project/cells";
import { ActionLink } from "@/components/site/action";
import { Band, SectionHeading, gutter } from "@/components/site/frame";
import { StaggerItem, StaggerList } from "@/components/site/motion-text";
import { PageHero } from "@/components/site/page-hero";
import { CONTRIBUTING_URL, DISCORD_URL, REPO_URL } from "@/lib/site";
import { cx } from "@/utils/cx";

export const metadata: Metadata = {
  title: "Community",
  description: "Join the W0rkTree community on Discord and GitHub.",
};

const channels = [
  { channel: "#general", description: "General discussion" },
  { channel: "#help", description: "Get support" },
  { channel: "#development", description: "Core development" },
  { channel: "#show-and-tell", description: "Share your work" },
];

const inlineLink =
  "text-text-primary underline decoration-text-tertiary underline-offset-4 transition-colors hover:decoration-current";

export default function CommunityPage() {
  return (
    <>
      <PageHero
        eyebrow="Open Source"
        title="Community."
        muted="Discord and GitHub."
        actions={
          <>
            <ActionLink href={DISCORD_URL} leadingIcon={RiDiscordLine} target="_blank" rel="noopener noreferrer">
              Join the Discord
            </ActionLink>
            <ActionLink
              href={REPO_URL}
              variant="secondary"
              leadingIcon={RiGithubLine}
              target="_blank"
              rel="noopener noreferrer"
            >
              View Repository
            </ActionLink>
          </>
        }
      >
        Join thousands of developers building the future of version control.
      </PageHero>

      {/* Discord: the channels as a server sidebar. */}
      <Band id="discord" frameClassName="bg-surface-sunken">
        <div className={cx("grid items-center gap-12 py-16 md:py-20 lg:grid-cols-12 lg:gap-10", gutter)}>
          <div className="flex flex-col items-start gap-8 lg:col-span-5">
            <SectionHeading index="01" eyebrow="Discord" title="Discord">
              Our Discord server is the best place to get help, share what you&apos;re building, and
              connect with the W0rkTree team and community in real time.
            </SectionHeading>
            <ActionLink
              href={DISCORD_URL}
              leadingIcon={RiDiscordLine}
              trailingIcon={RiArrowRightUpLine}
              target="_blank"
              rel="noopener noreferrer"
            >
              Join the Discord
            </ActionLink>
          </div>
          <Reveal className="min-w-0 lg:col-span-6 lg:col-start-7">
            <DiagramFrame label="discord.gg/worktree" live bodyClassName="p-2 sm:p-3">
              <StaggerList className="flex flex-col" delay={0.1}>
                {channels.map((item) => (
                  <StaggerItem
                    key={item.channel}
                    className="group/channel flex items-center gap-3 rounded-xl px-3 py-3 transition-colors duration-200 hover:bg-background-secondary-default"
                  >
                    <RiHashtag
                      className="size-4 shrink-0 text-foreground-icon-tertiary transition-colors group-hover/channel:text-foreground-icon-primary"
                      aria-hidden
                    />
                    <span className="font-mono text-body-2-medium">{item.channel.slice(1)}</span>
                    <span className="ms-auto text-right text-body-2-regular text-text-secondary">
                      {item.description}
                    </span>
                  </StaggerItem>
                ))}
              </StaggerList>
            </DiagramFrame>
          </Reveal>
        </div>
      </Band>

      {/* GitHub: the repository and the contribution guidelines. */}
      <Band id="github">
        <div className={cx("grid gap-12 py-16 md:py-20 lg:grid-cols-12 lg:gap-10", gutter)}>
          <div className="flex flex-col items-start gap-8 lg:col-span-5">
            <SectionHeading index="02" eyebrow="GitHub" title="GitHub">
              W0rkTree is developed in the open. Star the repo, browse the source, and contribute to
              the project on GitHub.
            </SectionHeading>
            <ActionLink
              href={REPO_URL}
              variant="secondary"
              leadingIcon={RiGithubLine}
              trailingIcon={RiArrowRightUpLine}
              target="_blank"
              rel="noopener noreferrer"
            >
              View Repository
            </ActionLink>
          </div>
          <Reveal className="min-w-0 lg:col-span-6 lg:col-start-7 lg:self-end">
            <div className="relative overflow-hidden rounded-2xl border border-border-button-default bg-surface-raised p-6 shadow-sm sm:p-8">
              <div aria-hidden className="hatch pointer-events-none absolute inset-y-0 right-0 w-1/3 opacity-70" />
              <p className="eyebrow relative">Contribution Guidelines</p>
              <p className="relative mt-4 text-body-regular leading-6 text-pretty text-text-secondary">
                Before opening a pull request, please read our{" "}
                <a href={CONTRIBUTING_URL} target="_blank" rel="noopener noreferrer" className={inlineLink}>
                  contribution guidelines
                </a>{" "}
                to understand the development workflow, coding standards, and review process.
              </p>
            </div>
          </Reveal>
        </div>
      </Band>

      <Band id="resources">
        <div className={cx("py-14 md:py-16", gutter)}>
          <SectionHeading index="03" eyebrow="Resources" title="Resources" />
        </div>
        <LinkGrid
          items={[
            {
              icon: RiArticleLine,
              title: "Articles",
              body: "Deep dives into W0rkTree's architecture, design decisions, and the future of version control.",
              href: "/articles",
            },
            {
              icon: RiBookOpenLine,
              title: "Guides",
              body: "Step-by-step tutorials to help you get started and master W0rkTree's workflows.",
              href: "/guides",
            },
            {
              icon: RiTeamLine,
              title: "Maintainers",
              body: "Meet the people behind W0rkTree and learn about the project governance.",
              href: "/maintainers",
            },
          ]}
        />
      </Band>

      {/* Closing call to contribute, on drifting hatch. */}
      <Band id="contributing" marks={false}>
        <div className={cx("relative overflow-hidden py-16 md:py-20", gutter)}>
          <div aria-hidden className="hatch hatch-drift pointer-events-none absolute inset-0 opacity-60" />
          <Reveal className="relative flex max-w-2xl flex-col items-start gap-6">
            <span className="flex size-10 items-center justify-center rounded-2lg border border-border-button-default bg-background-primary-default text-foreground-icon-primary shadow-xs">
              <RiHeartLine className="size-5" aria-hidden />
            </span>
            <h2 className="text-display-4-semibold tracking-[-0.03em] text-balance md:text-display-3-semibold md:leading-[1.12]">
              Contributing
            </h2>
            <p className="text-headline-regular leading-7 text-pretty text-text-secondary">
              W0rkTree thrives because of contributions from developers like you. Whether it&apos;s
              fixing a typo, improving documentation, or building a new feature, every contribution
              matters.
            </p>
            <ActionLink href="/contributing" leadingIcon={RiHeartLine} trailingIcon={RiArrowRightLine}>
              Start Contributing
            </ActionLink>
          </Reveal>
        </div>
      </Band>
    </>
  );
}
