import type { Metadata } from "next";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  IoMail,
  IoShieldCheckmark,
  IoChatbubbleEllipses,
  IoLogoGithub,
} from "react-icons/io5";
import { DISCORD_URL, ISSUES_URL, REPO_URL } from "@/lib/site";

export const metadata: Metadata = {
  title: "Contact - W0rkTree",
  description: "Get in touch with the W0rkTree team.",
};

const contacts = [
  {
    icon: IoMail,
    title: "General Inquiries",
    description: "For questions, feedback, and partnerships.",
    link: "mailto:hello@worktree.dev",
    linkLabel: "hello@worktree.dev",
  },
  {
    icon: IoShieldCheckmark,
    title: "Security Reports",
    description: "Report vulnerabilities privately, never in a public issue.",
    link: `${REPO_URL}/security/advisories/new`,
    linkLabel: "Open a private advisory",
  },
  {
    icon: IoChatbubbleEllipses,
    title: "Community",
    description: "Chat with the team and community on Discord.",
    link: DISCORD_URL,
    linkLabel: "Join Discord",
  },
  {
    icon: IoLogoGithub,
    title: "Bug Reports",
    description: "Report bugs and request features on GitHub.",
    link: ISSUES_URL,
    linkLabel: "Open an Issue",
  },
];

export default function ContactPage() {
  return (
    <div className="container mx-auto max-w-3xl px-6 py-16">
      <section className="mb-12 text-center">
        <h1 className="text-4xl font-bold tracking-tight sm:text-5xl">Contact Us</h1>
        <p className="mt-4 text-lg text-muted-foreground">
          Have a question or want to get involved? Reach out through any of the
          channels below.
        </p>
      </section>

      <section className="grid gap-6 md:grid-cols-2">
        {contacts.map((contact) => {
          const Icon = contact.icon;
          const external = contact.link.startsWith("http");
          return (
            <Card key={contact.title}>
              <CardHeader>
                <div className="flex items-center gap-3">
                  <Icon className="h-6 w-6 text-primary" aria-hidden="true" />
                  <CardTitle className="text-lg">{contact.title}</CardTitle>
                </div>
              </CardHeader>
              <CardContent>
                <p className="mb-3 text-sm text-muted-foreground">{contact.description}</p>
                <a
                  href={contact.link}
                  className="text-sm font-medium text-primary underline underline-offset-4 hover:text-primary/80"
                  {...(external ? { target: "_blank", rel: "noopener noreferrer" } : {})}
                >
                  {contact.linkLabel}
                </a>
              </CardContent>
            </Card>
          );
        })}
      </section>
    </div>
  );
}
