import { guidesSource } from "@/lib/source";
import { notFound } from "next/navigation";
import { getMDXComponents } from "@/components/mdx";
import Link from "next/link";
import type { Metadata } from "next";
import { RiArrowRightLine, RiBookOpenLine } from "react-icons/ri";
import { ActionLink } from "@/components/site/action";
import { Band, gutter } from "@/components/site/frame";
import { PageHero, ProseBand } from "@/components/site/page-hero";
import { AsideLinks, EditOnGitHub, TableOfContents } from "@/components/content/aside";
import { ContentCell, ContentGrid, GridHeading } from "@/components/content/content-grid";
import { pad, readingMinutes } from "@/components/content/format";
import { PagerBand } from "@/components/content/pager";
import { cx } from "@/utils/cx";

type GuidePageData = ReturnType<typeof guidesSource.getPages>[number];
type TreeNode = (typeof guidesSource.pageTree)["children"][number];

type Section = { name: string; id: string; pages: GuidePageData[] };

/**
 * Guides in reading order, grouped by the separators in content/guides/meta.json ("---Getting Started---").
 * Pages the tree does not list are kept at the end under "More guides".
 */
function getSections(): Section[] {
  const byUrl = new Map(
    guidesSource
      .getPages()
      .filter((page) => page.url !== "/guides")
      .map((page) => [page.url, page]),
  );
  const sections: Section[] = [];
  let current: Section | undefined;

  const visit = (nodes: TreeNode[]) => {
    for (const node of nodes) {
      if (node.type === "separator") {
        const name = typeof node.name === "string" && node.name ? node.name : "Guides";
        current = { name, id: slugify(name), pages: [] };
        sections.push(current);
      } else if (node.type === "page") {
        const page = byUrl.get(node.url);
        if (!page) continue;
        byUrl.delete(node.url);
        if (!current) {
          current = { name: "Guides", id: "guides", pages: [] };
          sections.push(current);
        }
        current.pages.push(page);
      } else if (node.type === "folder") {
        visit(node.index ? [node.index, ...node.children] : node.children);
      }
    }
  };
  visit(guidesSource.pageTree.children);

  if (byUrl.size > 0) {
    sections.push({ name: "More guides", id: "more-guides", pages: [...byUrl.values()] });
  }
  return sections.filter((section) => section.pages.length > 0);
}

function slugify(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
}

function GuidesIndex() {
  const sections = getSections();
  const total = sections.reduce((sum, section) => sum + section.pages.length, 0);
  const numbers = new Map(
    sections.flatMap((section) => section.pages).map((page, index) => [page.url, index + 1]),
  );

  return (
    <>
      <PageHero
        eyebrow="Guides"
        title="Real tasks,"
        muted="walked through end to end."
        actions={
          <>
            <ActionLink href="/guides/quick-start" trailingIcon={RiArrowRightLine}>
              Start with the quick start
            </ActionLink>
            <ActionLink href="/docs" variant="secondary" leadingIcon={RiBookOpenLine}>
              Read the docs
            </ActionLink>
          </>
        }
        meta={`${total} guides in ${sections.length} sections`}
      >
        Tutorials and walkthroughs for working with W0rkTree day to day, collaborating with a team,
        and running your own server. For exact commands and APIs, the docs are the reference.
      </PageHero>

      <Band marks={false}>
        <nav
          aria-label="Guide sections"
          className={cx("flex flex-wrap gap-x-6 gap-y-2 py-4", gutter)}
        >
          {sections.map((section, index) => (
            <a
              key={section.id}
              href={`#${section.id}`}
              className="flex items-center gap-2 font-mono text-caption-1-regular text-text-secondary transition-colors hover:text-text-primary"
            >
              <span className="text-text-tertiary">{pad(index + 1)}</span>
              {section.name}
              <span className="text-text-tertiary">({section.pages.length})</span>
            </a>
          ))}
        </nav>
      </Band>

      {sections.map((section, sectionIndex) => (
        <Band key={section.id} id={section.id}>
          <GridHeading
            index={pad(sectionIndex + 1)}
            label={section.name}
            count={`${section.pages.length} ${section.pages.length === 1 ? "guide" : "guides"}`}
          />
          <ContentGrid count={section.pages.length}>
            {section.pages.map((page) => (
              <ContentCell
                key={page.url}
                href={page.url}
                label={
                  <>
                    <span className="text-text-primary">{pad(numbers.get(page.url) ?? 0)}</span>
                    <span aria-hidden>·</span>
                    <span>{readingMinutes(page.data.structuredData)} min read</span>
                  </>
                }
                title={page.data.title}
                summary={page.data.description}
              />
            ))}
          </ContentGrid>
        </Band>
      ))}
    </>
  );
}

async function GuidePage({ slug }: { slug: string[] }) {
  const page = guidesSource.getPage(slug);
  if (!page) notFound();

  const MDX = page.data.body;
  const sections = getSections();
  const ordered = sections.flatMap((section) => section.pages);
  const position = ordered.findIndex((entry) => entry.url === page.url);
  const section = sections.find((entry) => entry.pages.some((p) => p.url === page.url));
  const previous = position > 0 ? ordered[position - 1] : undefined;
  const next = position >= 0 ? ordered[position + 1] : undefined;
  const minutes = readingMinutes(page.data.structuredData);

  return (
    <>
      <PageHero
        eyebrow={
          <>
            <Link href="/guides" className="underline-offset-4 hover:text-text-primary hover:underline">
              Guides
            </Link>
            {section ? (
              <>
                {" / "}
                <Link
                  href={`/guides#${section.id}`}
                  className="underline-offset-4 hover:text-text-primary hover:underline"
                >
                  {section.name}
                </Link>
              </>
            ) : null}
          </>
        }
        title={page.data.title}
        meta={
          <span className="font-mono">
            {minutes} min read
            {position >= 0 ? ` · Guide ${pad(position + 1)} of ${pad(ordered.length)}` : null}
          </span>
        }
      >
        {page.data.description}
      </PageHero>

      <ProseBand
        aside={
          <>
            <TableOfContents toc={page.data.toc} />
            {section ? (
              <AsideLinks
                label={section.name}
                current={page.url}
                links={section.pages.map((p) => ({ href: p.url, title: p.data.title }))}
              />
            ) : null}
            <EditOnGitHub path={`guides/${page.path}`} />
          </>
        }
      >
        <MDX components={getMDXComponents()} />
      </ProseBand>

      <PagerBand
        previous={
          previous
            ? { href: previous.url, title: previous.data.title, detail: previous.data.description }
            : undefined
        }
        next={
          next ? { href: next.url, title: next.data.title, detail: next.data.description } : undefined
        }
      />
    </>
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
  if (!page) return { title: "Guides" };

  return {
    // The index page is titled "Guides" itself.
    title: params.slug?.length ? `${page.data.title}: Guides` : "Guides",
    description: page.data.description,
  };
}
