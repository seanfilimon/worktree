import type { ReactNode } from "react";
import Link from "next/link";
import { RiArrowRightUpLine, RiGithubLine } from "react-icons/ri";
import { sourceFileUrl } from "@/lib/shared";
import { cx } from "@/utils/cx";

/**
 * Pieces of the sticky column beside long-form content (ProseBand's `aside`): a labelled block, the table of
 * contents, and the edit-on-GitHub link.
 */
export function AsideBlock({
  label,
  children,
  className,
}: {
  label: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={cx("flex flex-col gap-3 border-t border-separator-border pt-4 first:border-t-0 first:pt-0", className)}>
      <h2 className="eyebrow">{label}</h2>
      {children}
    </section>
  );
}

type TocItem = { title: ReactNode; url: string; depth: number };

/** "On this page": second- and third-level headings on a hairline rail. */
export function TableOfContents({ toc }: { toc: TocItem[] | undefined }) {
  const items = (toc ?? []).filter((item) => item.depth <= 3);
  if (items.length === 0) return null;
  return (
    <AsideBlock label="On this page">
      <nav aria-label="On this page">
        <ul className="flex flex-col border-l border-separator-border">
          {items.map((item) => (
            <li key={item.url}>
              <a
                href={item.url}
                className={cx(
                  "-ml-px block border-l border-transparent py-1 text-body-2-regular leading-5 text-text-secondary transition-colors hover:border-text-primary hover:text-text-primary",
                  item.depth > 2 ? "pl-6" : "pl-3",
                )}
              >
                {item.title}
              </a>
            </li>
          ))}
        </ul>
      </nav>
    </AsideBlock>
  );
}

/** A list of sibling links (other guides in the section, other maintainers), the current one marked. */
export function AsideLinks({
  label,
  links,
  current,
}: {
  label: string;
  links: { href: string; title: string }[];
  current?: string;
}) {
  if (links.length === 0) return null;
  return (
    <AsideBlock label={label}>
      <ul className="flex flex-col gap-1.5">
        {links.map((link) => (
          <li key={link.href}>
            {link.href === current ? (
              <span aria-current="page" className="text-body-2-medium text-text-primary">
                {link.title}
              </span>
            ) : (
              <Link
                href={link.href}
                className="text-body-2-regular text-text-secondary underline-offset-4 transition-colors hover:text-text-primary hover:underline"
              >
                {link.title}
              </Link>
            )}
          </li>
        ))}
      </ul>
    </AsideBlock>
  );
}

/** "Edit this page on GitHub" for a file under apps/web/content. */
export function EditOnGitHub({ path }: { path: string }) {
  return (
    <div className="border-t border-separator-border pt-4">
      <a
        href={sourceFileUrl(`apps/web/content/${path}`)}
        target="_blank"
        rel="noopener noreferrer"
        className="group/edit inline-flex w-fit items-center gap-1.5 text-caption-1-regular text-text-secondary transition-colors hover:text-text-primary"
      >
        <RiGithubLine className="size-4" aria-hidden />
        Edit this page on GitHub
        <RiArrowRightUpLine
          className="size-3.5 transition-transform duration-200 group-hover/edit:translate-x-0.5 group-hover/edit:-translate-y-0.5"
          aria-hidden
        />
      </a>
    </div>
  );
}
