import { useContext, useEffect, useId, useMemo, type ReactNode } from "react";
import { Breadcrumbs } from "./shell/Breadcrumbs";
import { HeaderTrailContext, type Crumb } from "./shell/headerTrail";

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

  const registry = useContext(HeaderTrailContext);
  const id = useId();
  // Callers pass fresh arrays each render; key the trail on its content.
  const crumbsKey = JSON.stringify(breadcrumbs ?? []);
  const trail = useMemo(() => ({ crumbs: JSON.parse(crumbsKey) as Crumb[], title: docTitle }), [crumbsKey, docTitle]);

  useEffect(() => {
    if (!registry) return;
    registry.set(id, trail);
    return () => registry.set(id, null);
  }, [registry, id, trail]);

  return (
    <header className="mb-6 flex flex-col gap-3">
      {!registry && <Breadcrumbs crumbs={trail.crumbs} />}
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
