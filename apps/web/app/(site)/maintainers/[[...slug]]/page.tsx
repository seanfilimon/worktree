import { maintainersSource } from "@/lib/source";
import { notFound } from "next/navigation";
import { getMDXComponents } from "@/components/mdx";
import Link from "next/link";
import type { Metadata } from "next";
import { RiArrowRightLine, RiGithubLine, RiTwitterXLine } from "react-icons/ri";
import { ActionLink } from "@/components/site/action";
import { Band } from "@/components/site/frame";
import { PageHero, ProseBand } from "@/components/site/page-hero";
import { AsideBlock, AsideLinks, EditOnGitHub, TableOfContents } from "@/components/content/aside";
import { Avatar } from "@/components/content/avatar";
import { ContentCell, ContentGrid, GridHeading } from "@/components/content/content-grid";
import { REPO_URL } from "@/lib/site";

type MaintainerPageData = ReturnType<typeof maintainersSource.getPages>[number];

function profiles(): MaintainerPageData[] {
  return maintainersSource.getPages().filter((p) => p.url !== "/maintainers");
}

function nameOf(page: MaintainerPageData): string {
  return page.data.name || page.data.title;
}

/** The profile grid on the index and under each profile. */
function MaintainerGrid({ pages }: { pages: MaintainerPageData[] }) {
  return (
    <ContentGrid count={pages.length}>
      {pages.map((page) => {
        const name = nameOf(page);
        return (
          <ContentCell
            key={page.url}
            href={page.url}
            media={<Avatar name={name} src={page.data.avatar} size="lg" className="mb-3" />}
            label={page.data.role}
            title={name}
            summary={page.data.bio}
            footer={
              page.data.github ? (
                <span className="font-mono">github.com/{page.data.github}</span>
              ) : undefined
            }
          />
        );
      })}
    </ContentGrid>
  );
}

function MaintainersIndex() {
  const pages = profiles();

  return (
    <>
      <PageHero
        eyebrow="Maintainers"
        title="The people building"
        muted="W0rkTree."
        actions={
          <>
            <ActionLink href="/contributing" trailingIcon={RiArrowRightLine}>
              Start contributing
            </ActionLink>
            <ActionLink href={REPO_URL} variant="secondary" leadingIcon={RiGithubLine}>
              View the repository
            </ActionLink>
          </>
        }
        meta={<span className="font-mono">{pages.length} profiles</span>}
      >
        W0rkTree is built in the open. Meet the maintainers who design, review, and ship it, and
        the community that contributes alongside them.
      </PageHero>

      <Band>
        <GridHeading index="01" label="Profiles" count={`${pages.length} total`} />
        <MaintainerGrid pages={pages} />
      </Band>
    </>
  );
}

async function MaintainerPage({ slug }: { slug: string[] }) {
  const page = maintainersSource.getPage(slug);
  if (!page) notFound();

  const MDX = page.data.body;
  const name = nameOf(page);
  const role = page.data.role;
  const github = page.data.github;
  const twitter = page.data.twitter;
  const all = profiles();
  const others = all.filter((p) => p.url !== page.url);

  return (
    <>
      <PageHero
        eyebrow={
          <>
            <Link
              href="/maintainers"
              className="underline-offset-4 hover:text-text-primary hover:underline"
            >
              Maintainers
            </Link>
            {role ? ` / ${role}` : null}
          </>
        }
        title={name}
        actions={
          github || twitter ? (
            <>
              {github ? (
                <ActionLink
                  href={`https://github.com/${github}`}
                  variant="secondary"
                  leadingIcon={RiGithubLine}
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  @{github}
                </ActionLink>
              ) : null}
              {twitter ? (
                <ActionLink
                  href={`https://x.com/${twitter}`}
                  variant="secondary"
                  leadingIcon={RiTwitterXLine}
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  @{twitter}
                </ActionLink>
              ) : null}
            </>
          ) : undefined
        }
      >
        {page.data.bio || page.data.description}
      </PageHero>

      <ProseBand
        aside={
          <>
            <AsideBlock label="Profile">
              <div className="flex items-center gap-3">
                <Avatar name={name} src={page.data.avatar} size="md" />
                <div className="flex min-w-0 flex-col">
                  <span className="text-body-2-medium text-text-primary">{name}</span>
                  {role ? (
                    <span className="text-caption-1-regular text-text-secondary">{role}</span>
                  ) : null}
                </div>
              </div>
            </AsideBlock>
            <TableOfContents toc={page.data.toc} />
            <AsideLinks
              label="Maintainers"
              current={page.url}
              links={all.map((p) => ({ href: p.url, title: nameOf(p) }))}
            />
            <EditOnGitHub path={`maintainers/${page.path}`} />
          </>
        }
      >
        <MDX components={getMDXComponents()} />
      </ProseBand>

      {others.length > 0 ? (
        <Band>
          <GridHeading label="More maintainers" count={`${others.length}`} />
          <MaintainerGrid pages={others} />
        </Band>
      ) : null}
    </>
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
  if (!page) return { title: "Maintainers" };
  return {
    title: page.data.name || page.data.title,
    description: page.data.description,
  };
}
