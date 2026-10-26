'use client';

import { useEffect, useState } from 'react';
import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { motion } from 'motion/react';
import {
  RiArrowRightLine,
  RiArrowRightUpLine,
  RiCloseLine,
  RiGithubFill,
  RiMenuLine,
} from 'react-icons/ri';
import { buttonStyles } from '@/components/base/buttons/button';
import { IconButton } from '@/components/base/buttons/icon-button';
import { ThemeToggle } from '@/components/application/theme/theme-toggle';
import { Logo } from '@/components/logo';
import { repositoryUrl } from '@/lib/shared';
import { cx } from '@/utils/cx';
import { Frame, gutter } from './frame';
import { SearchButton, SearchIconButton } from './search-button';

export const navLinks: { label: string; href: string; match: (path: string) => boolean }[] = [
  { label: 'Docs', href: '/docs', match: (path) => path.startsWith('/docs') },
  { label: 'Guides', href: '/guides', match: (path) => path.startsWith('/guides') },
  { label: 'Features', href: '/features', match: (path) => path === '/features' },
  { label: 'Articles', href: '/articles', match: (path) => path.startsWith('/articles') },
  { label: 'Changelog', href: '/changelog', match: (path) => path === '/changelog' },
];

/** The marketing header: sticky, hairline below, and inside the same rails as every band on the page. */
export function SiteHeader() {
  const pathname = usePathname();
  const [open, setOpen] = useState(false);
  const [hovered, setHovered] = useState<string | null>(null);

  useEffect(() => setOpen(false), [pathname]);
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => event.key === 'Escape' && setOpen(false);
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open]);

  return (
    <header className="sticky top-0 z-40 border-b border-separator-border bg-background-full/80 backdrop-blur-md">
      <Frame className={cx('flex h-14 items-center gap-6', gutter)}>
        <Link
          href="/"
          aria-label="W0rkTree home"
          className="flex h-8 shrink-0 items-center rounded-sm"
        >
          <Logo />
        </Link>
        {/* Links from lg and the full search box from xl: measured with the production GitHub button, the row
            overflows below those widths, so smaller screens get the menu button and the search icon instead. */}
        <nav
          aria-label="Main"
          className="hidden items-center gap-0.5 lg:flex"
          onPointerLeave={() => setHovered(null)}
        >
          {navLinks.map((link) => {
            const active = link.match(pathname);
            return (
              <Link
                key={link.href}
                href={link.href}
                aria-current={active ? 'page' : undefined}
                onPointerEnter={() => setHovered(link.href)}
                onFocus={() => setHovered(link.href)}
                onBlur={() => setHovered(null)}
                className={cx(
                  'relative isolate rounded-lg px-2.5 py-1.5 text-body-medium transition-colors duration-150',
                  active || hovered === link.href ? 'text-text-primary' : 'text-text-secondary',
                )}
              >
                {/* One pill glides to whichever link is under the pointer. */}
                {hovered === link.href ? (
                  <motion.span
                    layoutId="site-nav-hover"
                    aria-hidden
                    className="absolute inset-0 -z-10 rounded-lg bg-background-secondary-default"
                    transition={{ type: 'spring', stiffness: 500, damping: 38 }}
                  />
                ) : null}
                {link.label}
                {active ? (
                  <span
                    aria-hidden
                    // Sits on the header's bottom hairline (link is 32px tall, centered in 56px).
                    className="absolute inset-x-2.5 -bottom-[12px] h-px bg-text-primary"
                  />
                ) : null}
              </Link>
            );
          })}
        </nav>
        <div className="ms-auto flex items-center gap-2">
          <SearchButton className="hidden w-52 xl:inline-flex" />
          <SearchIconButton className="xl:hidden" />
          <ThemeToggle size="sm" className="hidden sm:inline-flex" />
          <GithubButton className="hidden sm:inline-flex" />
          <IconButton
            icon={open ? RiCloseLine : RiMenuLine}
            size="small"
            aria-label={open ? 'Close menu' : 'Open menu'}
            aria-expanded={open}
            aria-controls="site-menu"
            onClick={() => setOpen((value) => !value)}
            className="lg:hidden"
          />
        </div>
      </Frame>

      {open ? (
        <div id="site-menu" className="border-t border-separator-border lg:hidden">
          <Frame className={cx('flex flex-col gap-1 py-4', gutter)}>
            {navLinks.map((link) => (
              <Link
                key={link.href}
                href={link.href}
                className="flex items-center justify-between rounded-lg px-2 py-2.5 text-headline-medium text-text-primary hover:bg-background-secondary-default"
              >
                {link.label}
                <RiArrowRightLine className="size-4 text-foreground-icon-tertiary" aria-hidden />
              </Link>
            ))}
            <div className="mt-3 flex items-center justify-between gap-3 border-t border-separator-border pt-4">
              <ThemeToggle size="sm" />
              <GithubButton />
            </div>
          </Frame>
        </div>
      ) : null}
    </header>
  );
}

/** The header's call to action: the repository on GitHub, as a BoardUI primary button. */
function GithubButton({ className }: { className?: string }) {
  return (
    <a
      href={repositoryUrl}
      target="_blank"
      rel="noreferrer noopener"
      className={cx(
        buttonStyles.base,
        buttonStyles.size.small,
        buttonStyles.variant.primary,
        'group/github gap-1.5 px-2.5',
        className,
      )}
    >
      <RiGithubFill className={buttonStyles.icon.small} aria-hidden />
      <span className={buttonStyles.label.small}>GitHub</span>
      <RiArrowRightUpLine
        className="size-4 opacity-60 transition-[opacity,transform] duration-200 group-hover/github:translate-x-0.5 group-hover/github:-translate-y-0.5 group-hover/github:opacity-100"
        aria-hidden
      />
    </a>
  );
}
