import { articlesSource } from "@/lib/source";
import { notFound } from "next/navigation";
import { getMDXComponents } from "@/components/mdx";
import Link from "next/link";
import { Badge } from "@/components/ui/badge";
import type { Metadata } from "next";
import type { ReactNode } from "react";

function formatDate(date: string | Date): string {
  return new Date(date).toLocaleDateString("en-US", {
    year: "numeric",
    month: "long",
    day: "numeric",
  });
}

function ArticlesIndex() {
  const pages = articlesSource.getPages().sort((a, b) => {
    const dateA = new Date(a.data.date ?? 0).getTime();
    const dateB = new Date(b.data.date ?? 0).getTime();
    return dateB - dateA;
  });

  const featured = pages[0];
  const rest = pages.slice(1);

  return (
    <div className="w-full">
      {/* Hero */}
      <section className="border-b">
        <div className="container mx-auto max-w-7xl px-6 py-10 md:py-16">
          <div className="max-w-2xl">
            <Badge variant="outline" className="mb-3">
              {pages.length} articles
            </Badge>
            <h1 className="text-4xl font-bold tracking-tight md:text-5xl">
              Articles
            </h1>
            <p className="mt-3 text-lg text-muted-foreground">
              News, technical deep-dives, and updates from the W0rkTree team.
            </p>
          </div>
        </div>
      </section>

      <section className="container mx-auto max-w-7xl px-6 py-8">
        {/* Featured Article */}
        {featured && (
          <Link
            href={featured.url}
            className="group mb-8 block rounded-xl border border-border bg-card p-5 transition-all hover:border-foreground/20 hover:shadow-lg"
          >
            <Badge className="mb-3">Latest</Badge>
            <h2 className="text-2xl font-bold tracking-tight group-hover:text-primary transition-colors md:text-3xl">
              {featured.data.title}
            </h2>
            {(featured.data.description || featured.data.summary) && (
              <p className="mt-2 text-muted-foreground max-w-2xl">
                {featured.data.summary || featured.data.description}
              </p>
            )}
            <div className="mt-3 flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
              {featured.data.author && <span>By {featured.data.author}</span>}
              {featured.data.date && (
                <>
                  <span>·</span>
                  <time>{formatDate(featured.data.date)}</time>
                </>
              )}
              {featured.data.tags?.map((tag: string) => (
                <Badge key={tag} variant="secondary" className="text-xs">
                  {tag}
                </Badge>
              ))}
            </div>
          </Link>
        )}

        {/* Article List */}
        {rest.length > 0 && (
          <div className="space-y-0 divide-y divide-border">
            {rest.map((page) => (
              <Link
                key={page.url}
                href={page.url}
                className="group flex flex-col gap-0.5 py-4 transition-colors first:pt-0"
              >
                <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                  {page.data.date && <time>{formatDate(page.data.date)}</time>}
                  {page.data.author && (
                    <>
                      <span>·</span>
                      <span>{page.data.author}</span>
                    </>
                  )}
                  {page.data.tags?.map((tag: string) => (
                    <Badge key={tag} variant="outline" className="text-xs">
                      {tag}
                    </Badge>
                  ))}
                </div>
                <h3 className="text-base font-semibold tracking-tight group-hover:text-primary transition-colors">
                  {page.data.title}
                </h3>
                {(page.data.description || page.data.summary) && (
                  <p className="text-sm text-muted-foreground line-clamp-2">
                    {page.data.summary || page.data.description}
                  </p>
                )}
              </Link>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

async function ArticlePage({ slug }: { slug: string[] }) {
  const page = articlesSource.getPage(slug);
  if (!page) notFound();

  const MDX = page.data.body;

  return (
    <div className="w-full">
      {/* Article Header */}
      <section className="border-b">
        <div className="container mx-auto max-w-7xl px-6 py-12">
          <nav className="mb-6 flex items-center gap-2 text-sm text-muted-foreground">
            <Link
              href="/articles"
              className="hover:text-foreground transition-colors"
            >
              Articles
            </Link>
            <span>/</span>
            <span className="text-foreground truncate">{page.data.title}</span>
          </nav>
          <h1 className="text-3xl font-bold tracking-tight md:text-4xl max-w-3xl">
            {page.data.title}
          </h1>
          {page.data.description && (
            <p className="mt-3 text-lg text-muted-foreground max-w-2xl">
              {page.data.description}
            </p>
          )}
          <div className="mt-4 flex flex-wrap items-center gap-3 text-sm text-muted-foreground">
            {page.data.author && (
              <span className="font-medium text-foreground">
                {page.data.author}
              </span>
            )}
            {page.data.date && (
              <>
                <span>·</span>
                <time>{formatDate(page.data.date)}</time>
              </>
            )}
          </div>
          {page.data.tags && page.data.tags.length > 0 && (
            <div className="mt-3 flex gap-2">
              {page.data.tags.map((tag: string) => (
                <Badge key={tag} variant="secondary">
                  {tag}
                </Badge>
              ))}
            </div>
          )}
        </div>
      </section>

      {/* Content */}
      <div className="container mx-auto max-w-7xl px-6 py-12">
        <div className="flex gap-12">
          <article className="min-w-0 flex-1 max-w-3xl prose prose-neutral dark:prose-invert">
            <MDX components={getMDXComponents()} />
          </article>

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
    return <ArticlesIndex />;
  }

  return <ArticlePage slug={params.slug} />;
}

export async function generateStaticParams() {
  return articlesSource.generateParams();
}

export async function generateMetadata(props: {
  params: Promise<{ slug?: string[] }>;
}): Promise<Metadata> {
  const params = await props.params;
  const page = articlesSource.getPage(params.slug);
  if (!page) return { title: "Articles - W0rkTree" };

  return {
    title: `${page.data.title} - W0rkTree`,
    description: page.data.description,
  };
}
