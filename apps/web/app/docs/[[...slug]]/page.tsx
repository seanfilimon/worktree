import type { Metadata } from "next";
import { notFound } from "next/navigation";
import {
  DocsBody,
  DocsDescription,
  DocsPage,
  DocsTitle,
} from "fumadocs-ui/layouts/docs/page";
import { createRelativeLink } from "fumadocs-ui/mdx";
import { getMDXComponents } from "@/components/mdx";
import { CreatorCredit } from "@/components/site/creator-credit";
import { TocFooter } from "@/components/toc-footer";
import { docsSource } from "@/lib/source";

export default async function Page(props: {
  params: Promise<{ slug?: string[] }>;
}) {
  const params = await props.params;
  const page = docsSource.getPage(params.slug);
  if (!page) notFound();

  const MDX = page.data.body;

  return (
    <DocsPage
      toc={page.data.toc}
      full={page.data.full}
      tableOfContent={{
        style: "clerk",
        footer: (
          <TocFooter
            file={`apps/web/content/docs/${page.path}`}
            url={page.url}
          />
        ),
      }}
    >
      <DocsTitle>{page.data.title}</DocsTitle>
      <DocsDescription className="mb-0 border-b border-separator-border pb-6">
        {page.data.description}
      </DocsDescription>
      <DocsBody>
        <MDX
          components={getMDXComponents({
            a: createRelativeLink(docsSource, page),
          })}
        />
      </DocsBody>
      <div className="mt-8 flex flex-wrap items-center justify-between gap-3 border-t border-separator-border pt-6">
        <CreatorCredit label="W0rkTree is created by" />
      </div>
    </DocsPage>
  );
}

export async function generateStaticParams() {
  return docsSource.generateParams();
}

export async function generateMetadata(props: {
  params: Promise<{ slug?: string[] }>;
}): Promise<Metadata> {
  const params = await props.params;
  const page = docsSource.getPage(params.slug);
  if (!page) notFound();

  return {
    title: page.data.title,
    description: page.data.description,
  };
}
