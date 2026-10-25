import { useEffect, type ReactNode } from "react";
import { Breadcrumbs } from "./shell/Breadcrumbs";
import type { Crumb } from "./shell/headerTrail";
import { usePageTrail } from "./shell/usePageTrail";

export type { Crumb };

interface PageHeaderProps {
  title: ReactNode;
  /** Plain-text title for document.title when `title` is not a string. */
  documentTitle?: string;
  /** Shown in the app shell's header bar (inline when rendered outside it). */
  breadcrumbs?: Crumb[];
  description?: ReactNode;
  /** Buttons etc., right-aligned (wrapping below the title on narrow screens). */
  actions?: ReactNode;
}

export function PageHeader({ title, documentTitle, breadcrumbs, description, actions }: PageHeaderProps) {
  const docTitle = documentTitle ?? (typeof title === "string" ? title : undefined);
  useEffect(() => {
    if (docTitle) document.title = `${docTitle} · W0rkTree Admin`;
  }, [docTitle]);

  const { trail, inShell } = usePageTrail(breadcrumbs, docTitle);

  return (
    <header className="mb-6 flex flex-col gap-3">
      {!inShell && <Breadcrumbs crumbs={trail.crumbs} />}
      <div className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
        <div className="min-w-0 space-y-1">
          <h1 className="truncate text-xl font-semibold tracking-tight sm:text-2xl">{title}</h1>
          {description && <div className="text-sm text-muted-foreground">{description}</div>}
        </div>
        {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
      </div>
    </header>
  );
}
