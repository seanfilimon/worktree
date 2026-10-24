import { guidesSource } from "@/lib/source";
import { notFound } from "next/navigation";
import { getMDXComponents } from "@/components/mdx";
import Link from "next/link";
import { Badge } from "@/components/ui/badge";
import type { Metadata } from "next";
import type { ReactNode } from "react";

function getCategory(url: string): string {
  if (url.includes("/admin")) return "Admin Panel";
  if (url.includes("/quick-start")) return "Getting Started";
  if (url.includes("/migration")) return "Migration";
  if (url.includes("/architecture")) return "Architecture";
  if (url.includes("/versioning")) return "Reference";
  return "Guide";
}

function GuidesIndex() {
  const pages = guidesSource.getPages().filter((p) => p.url !== "/guides");

  return (
    <div className="w-full">
      {/* Hero */}
      <section className="border-b">
        <div className="container mx-auto max-w-7xl px-6 py-16 md:py-24">
          <div className="max-w-2xl">
            <Badge variant="outline" className="mb-4">
              {pages.length} guides available
            </Badge>
            <h1 className="text-4xl font-bold tracking-tight md:text-5xl">
              Guides
            </h1>
            <p className="mt-4 text-lg text-muted-foreground">
              Tutorials, walkthroughs, and reference material to help you get
              the most out of W0rkTree.
            </p>
          </div>
        </div>
      </section>

      {/* Guide Cards Grid */}
      <section className="container mx-auto max-w-7xl px-6 py-12">
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {pages.map((page) => (
            <Link
              key={page.url}
              href={page.url}
              className="group relative flex flex-col rounded-xl border border-border bg-card p-6 transition-all hover:border-foreground/20 hover:shadow-md"
            >
              <Badge variant="secondary" className="mb-3 w-fit text-xs">
                {getCategory(page.url)}
              </Badge>
              <h3 className="text-lg font-semibold tracking-tight group-hover:text-primary transition-colors">
                {page.data.title}
              </h3>
              {page.data.description && (
                <p className="mt-2 text-sm text-muted-foreground line-clamp-2">
                  {page.data.description}
                </p>
              )}
              <div className="mt-auto pt-4">
                <span className="text-xs font-medium text-muted-foreground group-hover:text-primary transition-colors">
                  Read guide →
                </span>
              </div>
            </Link>
          ))}
        </div>
      </section>
    </div>
  );
}

async function GuidePage({ slug }: { slug: string[] }) {
  const page = guidesSource.getPage(slug);
  if (!page) notFound();

  const MDX = page.data.body;

  return (
    <div className="w-full">
      {/* Header */}
      <section className="border-b">
        <div className="container mx-auto max-w-7xl px-6 py-12">
          <nav className="mb-6 flex items-center gap-2 text-sm text-muted-foreground">
            <Link
              href="/guides"
              className="hover:text-foreground transition-colors"
            >
              Guides
            </Link>
            <span>/</span>
            <span className="text-foreground">{page.data.title}</span>
          </nav>
          <h1 className="text-3xl font-bold tracking-tight md:text-4xl">
            {page.data.title}
          </h1>
          {page.data.description && (
            <p className="mt-3 text-lg text-muted-foreground max-w-2xl">
              {page.data.description}
            </p>
          )}
        </div>
      </section>

      {/* Content */}
      <div className="container mx-auto max-w-7xl px-6 py-12">
        <div className="flex gap-12">
          {/* Main content */}
          <article className="min-w-0 flex-1 max-w-4xl prose prose-neutral dark:prose-invert">
            <MDX components={getMDXComponents()} />
          </article>

          {/* TOC sidebar */}
          {page.data.toc && page.data.toc.length > 0 && (
            <aside className="hidden xl:block w-64 shrink-0">
              <div className="sticky top-24">
                <h4 className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                  On this page
                </h4>
                <nav className="space-y-1">
                  {page.data.toc.map(
                    (item: {
                      title: ReactNode;
                      url: string;
                      depth: number;
                    }) => (
                      <a
                        key={item.url}
                        href={item.url}
                        className={`block text-sm text-muted-foreground hover:text-foreground transition-colors ${
                          item.depth > 2 ? "pl-4" : ""
                        }`}
                      >
                        {item.title}
                      </a>
                    ),
                  )}
                </nav>
              </div>
            </aside>
          )}
        </div>
      </div>
    </div>
  );
}

export default async function Page(props: {
  params: Promise<{ slug?: string[] }>;
}) {
  const params = await props.params;

  if (!params.slug || params.slug.length === 0) {
    return <GuidesIndex />;
  }

  return <GuidePage slug={params.slug} />;
}

export async function generateStaticParams() {
  return guidesSource.generateParams();
}

export async function generateMetadata(props: {
  params: Promise<{ slug?: string[] }>;
}): Promise<Metadata> {
  const params = await props.params;
  const page = guidesSource.getPage(params.slug);
  if (!page) return { title: "Guides - W0rkTree" };

  return {
    title: `${page.data.title} - W0rkTree Guides`,
    description: page.data.description,
  };
}
