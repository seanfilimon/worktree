import type { Metadata } from "next";
import Link from "next/link";
import {
  IoLogoDiscord,
  IoLogoGithub,
  IoDocumentText,
  IoBook,
  IoPeople,
  IoHeart,
} from "react-icons/io5";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";

export const metadata: Metadata = {
  title: "Community - W0rkTree",
  description: "Join the W0rkTree community on Discord and GitHub.",
};

export default function CommunityPage() {
  return (
    <div className="container mx-auto max-w-4xl px-6 py-16">
      {/* Hero */}
      <section className="text-center pb-12 border-b">
        <Badge variant="secondary" className="mb-4">
          Open Source
        </Badge>
        <h1 className="text-4xl font-bold tracking-tight sm:text-5xl mb-4">
          Community
        </h1>
        <p className="text-lg text-muted-foreground max-w-2xl mx-auto">
          Join thousands of developers building the future of version control.
        </p>
      </section>

      {/* Discord Section */}
      <section className="py-12 border-b">
        <Card className="overflow-hidden">
          <CardHeader className="bg-[#5865F2]/10 pb-4">
            <div className="flex items-center gap-3">
              <IoLogoDiscord className="h-8 w-8 text-[#5865F2]" />
              <CardTitle className="text-2xl">Discord</CardTitle>
            </div>
          </CardHeader>
          <CardContent className="pt-6 space-y-4">
            <p className="text-muted-foreground">
              Our Discord server is the best place to get help, share what
              you&apos;re building, and connect with the W0rkTree team and
              community in real time.
            </p>
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
              {[
                { channel: "#general", description: "General discussion" },
                { channel: "#help", description: "Get support" },
                { channel: "#development", description: "Core development" },
                { channel: "#show-and-tell", description: "Share your work" },
              ].map((item) => (
                <div
                  key={item.channel}
                  className="rounded-lg border p-3 text-sm"
                >
                  <p className="font-medium font-mono">{item.channel}</p>
                  <p className="text-muted-foreground text-xs mt-1">
                    {item.description}
                  </p>
                </div>
              ))}
            </div>
            <Link
              href="https://discord.gg/worktree"
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-2 rounded-md bg-[#5865F2] px-4 py-2 text-sm font-medium text-white hover:bg-[#5865F2]/90 transition-colors"
            >
              <IoLogoDiscord className="h-4 w-4" />
              Join the Discord
            </Link>
          </CardContent>
        </Card>
      </section>

      {/* GitHub Section */}
      <section className="py-12 border-b">
        <Card className="overflow-hidden">
          <CardHeader className="bg-muted/50 pb-4">
            <div className="flex items-center gap-3">
              <IoLogoGithub className="h-8 w-8" />
              <CardTitle className="text-2xl">GitHub</CardTitle>
            </div>
          </CardHeader>
          <CardContent className="pt-6 space-y-4">
            <p className="text-muted-foreground">
              W0rkTree is developed in the open. Star the repo, browse the
              source, and contribute to the project on GitHub.
            </p>
            <div className="flex flex-wrap items-center gap-3">
              <Link
                href="https://github.com/worktree/worktree"
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
              >
                <IoLogoGithub className="h-4 w-4" />
                View Repository
              </Link>
              <Badge variant="outline" className="text-sm">
                ⭐ 2.4k stars
              </Badge>
            </div>
            <div className="rounded-lg border p-4 text-sm">
              <p className="font-medium mb-1">Contribution Guidelines</p>
              <p className="text-muted-foreground">
                Before opening a pull request, please read our{" "}
                <Link
                  href="https://github.com/worktree/worktree/blob/main/CONTRIBUTING.md"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="underline underline-offset-4 hover:text-foreground transition-colors"
                >
                  contribution guidelines
                </Link>{" "}
                to understand the development workflow, coding standards, and
                review process.
              </p>
            </div>
          </CardContent>
        </Card>
      </section>

      {/* Resources Links */}
      <section className="py-12 border-b">
        <h2 className="text-2xl font-bold tracking-tight mb-6">Resources</h2>
        <div className="grid gap-4 sm:grid-cols-3">
          <Link href="/articles" className="group">
            <Card className="h-full transition-colors group-hover:border-primary/50">
              <CardHeader>
                <IoDocumentText className="h-6 w-6 text-muted-foreground group-hover:text-primary transition-colors mb-2" />
                <CardTitle className="text-lg">Articles</CardTitle>
              </CardHeader>
              <CardContent>
                <p className="text-sm text-muted-foreground">
                  Deep dives into W0rkTree&apos;s architecture, design
                  decisions, and the future of version control.
                </p>
              </CardContent>
            </Card>
          </Link>

          <Link href="/guides" className="group">
            <Card className="h-full transition-colors group-hover:border-primary/50">
              <CardHeader>
                <IoBook className="h-6 w-6 text-muted-foreground group-hover:text-primary transition-colors mb-2" />
                <CardTitle className="text-lg">Guides</CardTitle>
              </CardHeader>
              <CardContent>
                <p className="text-sm text-muted-foreground">
                  Step-by-step tutorials to help you get started and master
                  W0rkTree&apos;s workflows.
                </p>
              </CardContent>
            </Card>
          </Link>

          <Link href="/maintainers" className="group">
            <Card className="h-full transition-colors group-hover:border-primary/50">
              <CardHeader>
                <IoPeople className="h-6 w-6 text-muted-foreground group-hover:text-primary transition-colors mb-2" />
                <CardTitle className="text-lg">Maintainers</CardTitle>
              </CardHeader>
              <CardContent>
                <p className="text-sm text-muted-foreground">
                  Meet the people behind W0rkTree and learn about the project
                  governance.
                </p>
              </CardContent>
            </Card>
          </Link>
        </div>
      </section>

      {/* Contributing */}
      <section className="py-12">
        <div className="flex items-start gap-4">
          <div className="rounded-full bg-pink-500/10 p-3">
            <IoHeart className="h-6 w-6 text-pink-500" />
          </div>
          <div>
            <h2 className="text-2xl font-bold tracking-tight mb-2">
              Contributing
            </h2>
            <p className="text-muted-foreground mb-4">
              W0rkTree thrives because of contributions from developers like
              you. Whether it&apos;s fixing a typo, improving documentation, or
              building a new feature, every contribution matters.
            </p>
            <Link
              href="/contributing"
              className="inline-flex items-center gap-2 rounded-md border px-4 py-2 text-sm font-medium hover:bg-muted transition-colors"
            >
              <IoHeart className="h-4 w-4" />
              Start Contributing
            </Link>
          </div>
        </div>
      </section>
    </div>
  );
}
