import Link from "next/link";
import { RiArrowRightLine, RiBookOpenLine } from "react-icons/ri";
import { ThemeToggle } from "@/components/application/theme/theme-toggle";
import { CreatorCredit } from "@/components/site/creator-credit";
import { version } from "@/lib/shared";

/** Bottom of the docs sidebar: the guides, the creator, the theme toggle (BoardUI), and version links. */
export function SidebarFooter() {
  return (
    <div className="flex flex-col gap-3 pt-1">
      <Link
        href="/guides"
        className="group flex items-center gap-2.5 rounded-xl border border-border-button-default bg-background-primary-default px-3 py-2 text-body-medium shadow-xs transition-colors hover:border-border-button-hover hover:bg-background-primary-hover"
      >
        <RiBookOpenLine
          className="size-4 text-foreground-icon-primary"
          aria-hidden
        />
        <span className="flex-1">Guides</span>
        <RiArrowRightLine
          className="size-4 text-foreground-icon-tertiary transition-transform group-hover:translate-x-0.5"
          aria-hidden
        />
      </Link>
      <CreatorCredit className="ps-1" />
      <div className="flex items-center gap-2">
        <ThemeToggle size="sm" />
        <span className="ms-auto flex min-w-0 items-center gap-1.5 truncate text-caption-1-regular text-text-secondary">
          <Link
            href="/changelog"
            title="Changelog"
            className="font-mono hover:text-text-primary"
          >
            v{version}
          </Link>
          ·
          <Link href="/roadmap" className="hover:text-text-primary">
            Roadmap
          </Link>
        </span>
      </div>
    </div>
  );
}
