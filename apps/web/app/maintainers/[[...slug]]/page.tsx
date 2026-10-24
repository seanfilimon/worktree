import { maintainersSource } from "@/lib/source";
import { notFound } from "next/navigation";
import { getMDXComponents } from "@/components/mdx";
import Link from "next/link";
import Image from "next/image";
import { Badge } from "@/components/ui/badge";
import type { Metadata } from "next";

function getInitials(name: string): string {
  return name
    .split(" ")
    .map((n) => n[0])
    .join("")
    .toUpperCase()
    .slice(0, 2);
}

function Avatar({
  name,
  avatar,
  size = "md",
}: {
  name: string;
  avatar?: string;
  size?: "sm" | "md" | "lg" | "xl";
}) {
  const sizeClasses = {
    sm: "h-12 w-12 text-sm",
    md: "h-16 w-16 text-lg",
    lg: "h-20 w-20 text-xl",
    xl: "h-24 w-24 text-2xl",
  };

  const cls = sizeClasses[size];

  if (avatar) {
    return (
      <Image
        src={avatar}
        alt={name}
        width={96}
        height={96}
        className={`${cls} shrink-0 rounded-full object-cover border-2 border-border`}
      />
    );
  }

  return (
    <div
      className={`${cls} shrink-0 flex items-center justify-center rounded-full bg-primary/10 font-bold text-primary`}
    >
      {getInitials(name)}
    </div>
  );
}

function MaintainersIndex() {
  const pages = maintainersSource
    .getPages()
    .filter((p) => p.url !== "/maintainers");

  return (
    <div className="w-full overflow-y-auto">
      {/* Hero */}
      <section className="border-b">
        <div className="container mx-auto max-w-7xl px-6 py-16 md:py-24">
          <div className="max-w-2xl">
            <h1 className="text-4xl font-bold tracking-tight md:text-5xl">
              Maintainers
            </h1>
            <p className="mt-4 text-lg text-muted-foreground">
              Meet the people building and maintaining W0rkTree.
            </p>
          </div>
        </div>
      </section>

      {/* Team Grid */}
      <section className="container mx-auto max-w-7xl px-6 py-12">
        <div className="grid gap-8 sm:grid-cols-2 lg:grid-cols-3">
          {pages.map((page) => {
            const name = page.data.name || page.data.title;
            const role = page.data.role;
            const bio = page.data.bio;
            const github = page.data.github;
            const avatar = page.data.avatar;

            return (
              <Link
                key={page.url}
                href={page.url}
                className="group flex flex-col items-center rounded-xl border border-border bg-card p-8 text-center transition-all hover:border-foreground/20 hover:shadow-lg"
              >
                <Avatar name={name} avatar={avatar} size="lg" />
                <div className="mt-5 min-w-0">
                  <h3 className="text-xl font-semibold tracking-tight group-hover:text-primary transition-colors">
                    {name}
                  </h3>
                  {role && (
                    <p className="mt-1 text-sm text-muted-foreground">{role}</p>
                  )}
                </div>
                {bio && (
                  <p className="mt-4 text-sm text-muted-foreground line-clamp-3">
                    {bio}
                  </p>
                )}
                {github && (
                  <div className="mt-auto pt-5">
                    <Badge variant="outline" className="text-xs">
                      @{github}
                    </Badge>
                  </div>
                )}
              </Link>
            );
          })}
        </div>
      </section>
    </div>
  );
}

async function MaintainerPage({ slug }: { slug: string[] }) {
  const page = maintainersSource.getPage(slug);
  if (!page) notFound();

  const MDX = page.data.body;
  const name = page.data.name || page.data.title;
  const role = page.data.role;
  const github = page.data.github;
  const avatar = page.data.avatar;

  return (
    <div className="w-full overflow-y-auto">
      <section className="border-b">
        <div className="container mx-auto max-w-7xl px-6 py-12">
          <nav className="mb-8 flex items-center gap-2 text-sm text-muted-foreground">
            <Link
              href="/maintainers"
              className="hover:text-foreground transition-colors"
            >
              Maintainers
            </Link>
            <span>/</span>
            <span className="text-foreground">{name}</span>
          </nav>

          <div className="flex flex-col items-center gap-6 sm:flex-row sm:items-start">
            <Avatar name={name} avatar={avatar} size="xl" />
            <div className="text-center sm:text-left">
              <h1 className="text-3xl font-bold tracking-tight md:text-4xl">
                {name}
              </h1>
              {role && (
                <p className="mt-2 text-lg text-muted-foreground">{role}</p>
              )}
              {github && (
                <a
                  href={`https://github.com/${github}`}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="mt-3 inline-flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors"
                >
                  <svg
                    className="h-4 w-4"
                    fill="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z" />
                  </svg>
                  @{github}
                </a>
              )}
            </div>
          </div>
        </div>
      </section>

      <div className="container mx-auto max-w-7xl px-6 py-12">
        <article className="max-w-3xl prose prose-neutral dark:prose-invert">
          <MDX components={getMDXComponents()} />
        </article>
      </div>
    </div>
  );
}

export default async function Page(props: {
  params: Promise<{ slug?: string[] }>;
}) {
  const params = await props.params;
  if (!params.slug || params.slug.length === 0) {
    return <MaintainersIndex />;
  }
  return <MaintainerPage slug={params.slug} />;
}

export async function generateStaticParams() {
  return maintainersSource.generateParams();
}

export async function generateMetadata(props: {
  params: Promise<{ slug?: string[] }>;
}): Promise<Metadata> {
  const params = await props.params;
  const page = maintainersSource.getPage(params.slug);
  if (!page) return { title: "Maintainers - W0rkTree" };
  return {
    title: `${page.data.name || page.data.title} - W0rkTree`,
    description: page.data.description,
  };
}
