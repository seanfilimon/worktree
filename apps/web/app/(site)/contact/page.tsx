import type { Metadata } from "next";
import { RiDiscordLine, RiGithubLine, RiMailLine, RiShieldCheckLine } from "react-icons/ri";
import { LinkGrid, type LinkCellItem } from "@/components/project/cells";
import { Band, SectionHeading, gutter } from "@/components/site/frame";
import { PageHero } from "@/components/site/page-hero";
import { DISCORD_URL, ISSUES_URL, REPO_URL } from "@/lib/site";
import { cx } from "@/utils/cx";

export const metadata: Metadata = {
  title: "Contact",
  description: "Get in touch with the W0rkTree team.",
};

const contacts: LinkCellItem[] = [
  {
    icon: RiMailLine,
    title: "General Inquiries",
    body: "For questions, feedback, and partnerships.",
    href: "mailto:hello@worktree.dev",
    label: "hello@worktree.dev",
  },
  {
    icon: RiShieldCheckLine,
    title: "Security Reports",
    body: "Report vulnerabilities privately, never in a public issue.",
    href: `${REPO_URL}/security/advisories/new`,
    label: "Open a private advisory",
  },
  {
    icon: RiDiscordLine,
    title: "Community",
    body: "Chat with the team and community on Discord.",
    href: DISCORD_URL,
    label: "Join Discord",
  },
  {
    icon: RiGithubLine,
    title: "Bug Reports",
    body: "Report bugs and request features on GitHub.",
    href: ISSUES_URL,
    label: "Open an Issue",
  },
];

export default function ContactPage() {
  return (
    <>
      <PageHero eyebrow="Contact" title="Contact Us." muted="Get in touch with the W0rkTree team.">
        Have a question or want to get involved? Reach out through any of the channels below.
      </PageHero>

      <Band id="channels">
        <div className={cx("py-14 md:py-16", gutter)}>
          <SectionHeading index="01" eyebrow="Channels" title="Pick the right channel" />
        </div>
        <LinkGrid columns={4} items={contacts} />
      </Band>
    </>
  );
}
