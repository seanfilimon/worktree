import Link from 'next/link';
import { ThemeToggle } from '@/components/application/theme/theme-toggle';
import { Logo } from '@/components/logo';
import { creator, repositoryUrl, version } from '@/lib/shared';
import { LICENSE_NAME } from '@/lib/site';
import { cx } from '@/utils/cx';
import { CreatorCredit } from './creator-credit';
import { Frame, gutter } from './frame';

const columns: { title: string; links: [string, string][] }[] = [
  {
    title: 'Learn',
    links: [
      ['Introduction', '/docs'],
      ['Installation', '/docs/installation'],
      ['Guides', '/guides'],
      ['Articles', '/articles'],
    ],
  },
  {
    title: 'Reference',
    links: [
      ['CLI', '/docs/cli'],
      ['Server', '/docs/server'],
      ['Protocol', '/docs/protocol'],
      ['SDK', '/docs/sdk'],
      ['Security', '/docs/security'],
    ],
  },
  {
    title: 'Project',
    links: [
      ['Features', '/features'],
      ['Roadmap', '/roadmap'],
      ['Changelog', '/changelog'],
      ['Maintainers', '/maintainers'],
      ['About', '/about'],
    ],
  },
  {
    title: 'Community',
    links: [
      ['Community', '/community'],
      ['Contributing', '/contributing'],
      ['Code of conduct', '/code-of-conduct'],
      ['Security policy', '/security'],
      ['Source code', repositoryUrl],
    ],
  },
];

export function SiteFooter() {
  return (
    <footer>
      <Frame>
        <div
          className={cx(
            'grid grid-cols-2 gap-x-6 gap-y-10 py-14 md:grid-cols-[minmax(0,1.4fr)_repeat(4,minmax(0,1fr))]',
            gutter,
          )}
        >
          <div className="col-span-2 flex flex-col gap-3 md:col-span-1">
            <Logo />
            <p className="max-w-xs text-body-regular leading-6 text-text-secondary">
              Version control with nested trees, automatic tracking and team-visible work in progress, on a server you run.
            </p>
            <CreatorCredit className="mt-1" />
          </div>
          {columns.map((column) => (
            <div key={column.title} className="flex flex-col gap-3">
              <p className="text-caption-1-semibold text-text-primary">{column.title}</p>
              <ul className="flex flex-col gap-2">
                {column.links.map(([label, href]) => {
                  const className =
                    'text-body-regular text-text-secondary underline decoration-transparent underline-offset-4 transition-[color,text-decoration-color] duration-200 hover:text-text-primary hover:decoration-current';
                  // Static files and other sites are ordinary links, not client-side routes.
                  const plain = /^https?:|\.(json|txt)$/.test(href);
                  return (
                    <li key={href}>
                      {plain ? (
                        <a href={href} className={className}>
                          {label}
                        </a>
                      ) : (
                        <Link href={href} className={className}>
                          {label}
                        </Link>
                      )}
                    </li>
                  );
                })}
              </ul>
            </div>
          ))}
        </div>
        <div
          className={cx(
            'flex flex-wrap items-center justify-between gap-4 border-t border-separator-border py-5',
            gutter,
          )}
        >
          <p className="text-caption-1-regular text-text-secondary">
            <span className="font-mono">v{version}</span> · {LICENSE_NAME} · ©{' '}
            {new Date().getFullYear()} <span className="text-text-primary">{creator}</span>
          </p>
          <div className="flex items-center gap-4">
            <Link
              href="/privacy"
              className="text-caption-1-regular text-text-secondary transition-colors hover:text-text-primary"
            >
              Privacy
            </Link>
            <Link
              href="/terms"
              className="text-caption-1-regular text-text-secondary transition-colors hover:text-text-primary"
            >
              Terms
            </Link>
            <ThemeToggle size="sm" />
          </div>
        </div>
      </Frame>
    </footer>
  );
}
