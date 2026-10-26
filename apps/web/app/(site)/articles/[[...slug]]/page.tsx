import { articlesSource, maintainersSource } from "@/lib/source";
import { notFound } from "next/navigation";
import { getMDXComponents } from "@/components/mdx";
import Link from "next/link";
import type { Metadata } from "next";
import { RiArrowRightLine, RiHistoryLine } from "react-icons/ri";
import { Callout } from "@/components/docs/callout";
import { TextLink } from "@/components/site/action";
import { Band, gutter } from "@/components/site/frame";
import { PageHero, ProseBand } from "@/components/site/page-hero";
import { AsideBlock, EditOnGitHub, TableOfContents } from "@/components/content/aside";
import { Avatar } from "@/components/content/avatar";
import {
  CellArrow,
  ContentCell,
  ContentGrid,
  GridHeading,
  HoverHatch,
  TagChips,
} from "@/components/content/content-grid";
import { formatDate, isoDate, readingMinutes } from "@/components/content/format";
import { PagerBand } from "@/components/content/pager";
import { CURRENT_VERSION, CURRENT_VERSION_DATE } from "@/lib/site";
import { cx } from "@/utils/cx";

type ArticlePageData = ReturnType<typeof articlesSource.getPages>[number];

/** Articles published before the current release may describe superseded designs. */
function predatesRebuild(date: string | Date | undefined): boolean {
  return date !== undefined && new Date(date) < new Date(CURRENT_VERSION_DATE);
}

/** Newest first. */
function sortedArticles(): ArticlePageData[] {
  return articlesSource.getPages().sort((a, b) => {
    const dateA = new Date(a.data.date ?? 0).getTime();
    const dateB = new Date(b.data.date ?? 0).getTime();
    return dateB - dateA;
  });
}

/**
 * The maintainer profile an article's byline refers to: an exact name match, or a shortened form of one
 * ("Sean F" for "Sean Filimon").
 */
function authorProfile(author: string) {
  const words = author.trim().split(/\s+/);
  return maintainersSource.getPages().find((page) => {
    const name = page.data.name ?? page.data.title;
    if (!name || page.url === "/maintainers") return false;
    if (name === author) return true;
    return words.length > 1 && name.startsWith(author);
  });
}

function summaryOf(page: ArticlePageData): string | undefined {
  return page.data.summary || page.data.description;
}

function ArticlesIndex() {
  const pages = sortedArticles();
  const featured = pages[0];
  const rest = pages.slice(1);
  const older = pages.filter((page) => predatesRebuild(page.data.date)).length;

  return (
    <>
      <PageHero
        eyebrow="Articles"
        title="Notes from building"
        muted="W0rkTree."
        meta={
          <span className="font-mono">
            {pages.length} articles
            {featured?.data.date ? ` · Latest ${formatDate(featured.data.date)}` : null}
          </span>
        }
      >
        Announcements, design notes, and technical deep dives from the people working on the
        project.
      </PageHero>

      {older > 0 ? (
        <Band marks={false}>
          <p
            className={cx(
              "flex flex-wrap items-center gap-x-2 gap-y-1 py-4 text-body-2-regular text-text-secondary",
              gutter,
            )}
          >
            <RiHistoryLine className="size-4 shrink-0 text-foreground-icon-secondary" aria-hidden />
            <span>
              {older === pages.length ? "Every article here" : `${older} of these articles`} predates
              the {CURRENT_VERSION} rebuild, so some designs may have changed.
            </span>
            <TextLink href="/docs" trailingIcon={RiArrowRightLine} className="text-body-2-medium">
              The docs describe the current release
            </TextLink>
          </p>
        </Band>
      ) : null}

      {featured ? <FeaturedArticle page={featured} /> : null}

      {rest.length > 0 ? (
        <Band>
          <GridHeading index="02" label="All articles" count={`${rest.length} more`} />
          <ContentGrid count={rest.length}>
            {rest.map((page) => (
              <ContentCell
                key={page.url}
                href={page.url}
                label={
                  <>
                    {page.data.date ? (
                      <time dateTime={isoDate(page.data.date)} className="text-text-primary">
                        {isoDate(page.data.date)}
                      </time>
                    ) : null}
                    <span aria-hidden>·</span>
                    <span>{readingMinutes(page.data.structuredData)} min read</span>
                  </>
                }
                title={page.data.title}
                summary={summaryOf(page)}
                tags={page.data.tags}
                footer={page.data.author ? `By ${page.data.author}` : undefined}
              />
            ))}
          </ContentGrid>
        </Band>
      ) : null}
    </>
  );
}

/** The newest article as one wide cell: date and title on the left, summary and tags on the right. */
function FeaturedArticle({ page }: { page: ArticlePageData }) {
  const summary = summaryOf(page);
  return (
    <Band>
      <GridHeading index="01" label="Latest" />
      <Link
        href={page.url}
        className={cx(
          "group relative grid gap-6 overflow-hidden py-10 transition-colors duration-200 hover:bg-background-secondary-default md:py-14 lg:grid-cols-12 lg:gap-12",
          gutter,
        )}
      >
        <HoverHatch />
        <div className="relative flex flex-col gap-4 lg:col-span-7">
          <p className="flex flex-wrap items-center gap-2 font-mono text-caption-1-regular text-text-tertiary">
            {page.data.date ? (
              <time dateTime={isoDate(page.data.date)} className="text-text-primary">
                {isoDate(page.data.date)}
              </time>
            ) : null}
            <span aria-hidden>·</span>
            <span>{readingMinutes(page.data.structuredData)} min read</span>
          </p>
          <h2 className="text-display-4-semibold tracking-[-0.025em] text-balance md:text-display-3-semibold md:leading-[1.12]">
            {page.data.title}
          </h2>
        </div>
        <div className="relative flex flex-col gap-5 lg:col-span-5 lg:justify-end">
          {summary ? (
            <p className="text-headline-regular leading-7 text-pretty text-text-secondary">
              {summary}
            </p>
          ) : null}
          {page.data.tags?.length ? <TagChips tags={page.data.tags} /> : null}
          <p className="flex items-center justify-between gap-4 border-t border-separator-border pt-4 text-caption-1-regular text-text-secondary">
            <span>{page.data.author ? `By ${page.data.author}` : null}</span>
            <span className="flex items-center gap-1.5 text-body-2-medium text-text-primary">
              Read the article
              <CellArrow />
            </span>
          </p>
        </div>
      </Link>
    </Band>
  );
}

async function ArticlePage({ slug }: { slug: string[] }) {
  const page = articlesSource.getPage(slug);
  if (!page) notFound();

  const MDX = page.data.body;
  const ordered = sortedArticles();
  const position = ordered.findIndex((entry) => entry.url === page.url);
  const newer = position > 0 ? ordered[position - 1] : undefined;
  const older = position >= 0 ? ordered[position + 1] : undefined;
  const profile = page.data.author ? authorProfile(page.data.author) : undefined;
  const minutes = readingMinutes(page.data.structuredData);
  const tags = page.data.tags ?? [];

  return (
    <>
      <PageHero
        eyebrow={
          <>
            <Link href="/articles" className="underline-offset-4 hover:text-text-primary hover:underline">
              Articles
            </Link>
            {page.data.date ? (
              <>
                {" / "}
                <time dateTime={isoDate(page.data.date)}>{isoDate(page.data.date)}</time>
              </>
            ) : null}
          </>
        }
        title={page.data.title}
        meta={
          <>
            {page.data.author ? (
              <>
                By{" "}
                {profile ? (
                  <Link
                    href={profile.url}
                    className="text-caption-1-medium text-text-primary underline-offset-4 hover:underline"
                  >
                    {page.data.author}
                  </Link>
                ) : (
                  <span className="text-caption-1-medium text-text-primary">{page.data.author}</span>
                )}
                {" · "}
              </>
            ) : null}
            {page.data.date ? (
              <>
                <time dateTime={isoDate(page.data.date)}>{formatDate(page.data.date)}</time>
                {" · "}
              </>
            ) : null}
            {minutes} min read
          </>
        }
      >
        {page.data.description || page.data.summary}
      </PageHero>

      <ProseBand
        aside={
          <>
            <TableOfContents toc={page.data.toc} />
            {tags.length > 0 ? (
              <AsideBlock label="Tags">
                <TagChips tags={tags} />
              </AsideBlock>
            ) : null}
            {page.data.author ? (
              <AsideBlock label="Written by">
                <AuthorCard
                  name={page.data.author}
                  href={profile?.url}
                  role={profile?.data.role}
                  avatar={profile?.data.avatar}
                />
              </AsideBlock>
            ) : null}
            <EditOnGitHub path={`articles/${page.path}`} />
          </>
        }
      >
        {predatesRebuild(page.data.date) && (
          <Callout type="warn" title={`Written before ${CURRENT_VERSION}`}>
            This article predates the {CURRENT_VERSION} rebuild. Some features it describes work
            differently today or are still on the <Link href="/roadmap">roadmap</Link>. The{" "}
            <Link href="/docs">documentation</Link> reflects the current release.
          </Callout>
        )}
        <MDX components={getMDXComponents()} />
        {tags.length > 0 ? (
          <div className="not-prose mt-10 border-t border-separator-border pt-6 lg:hidden">
            <TagChips tags={tags} />
          </div>
        ) : null}
      </ProseBand>

      <PagerBand
        previousLabel="Older"
        nextLabel="Newer"
        previous={
          older ? { href: older.url, title: older.data.title, detail: summaryOf(older) } : undefined
        }
        next={
          newer ? { href: newer.url, title: newer.data.title, detail: summaryOf(newer) } : undefined
        }
      />
    </>
  );
}

function AuthorCard({
  name,
  href,
  role,
  avatar,
}: {
  name: string;
  href?: string;
  role?: string;
  avatar?: string;
}) {
  const body = (
    <>
      <Avatar name={name} src={avatar} size="sm" />
      <span className="flex min-w-0 flex-col">
        <span className="text-body-2-medium text-text-primary">{name}</span>
        {role ? <span className="text-caption-1-regular text-text-secondary">{role}</span> : null}
      </span>
    </>
  );
  if (!href) return <div className="flex items-center gap-3">{body}</div>;
  return (
    <Link
      href={href}
      className="-m-2 flex items-center gap-3 rounded-lg p-2 transition-colors hover:bg-background-secondary-default"
    >
      {body}
    </Link>
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
  if (!page) return { title: "Articles" };

  return {
    title: page.data.title,
    description: page.data.description,
  };
}
