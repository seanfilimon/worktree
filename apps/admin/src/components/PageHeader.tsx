import { useEffect, type ReactNode } from "react";
import { Link } from "react-router";
import { Icon } from "./icons";

export interface Crumb {
  label: string;
  to?: string;
}

interface PageHeaderProps {
  title: ReactNode;
  /** Plain-text title for document.title when `title` is not a string. */
  documentTitle?: string;
  breadcrumbs?: Crumb[];
  description?: ReactNode;
  actions?: ReactNode;
}

export function PageHeader({ title, documentTitle, breadcrumbs = [], description, actions }: PageHeaderProps) {
  const docTitle = documentTitle ?? (typeof title === "string" ? title : undefined);
  useEffect(() => {
    if (docTitle) document.title = `${docTitle} · W0rkTree Admin`;
  }, [docTitle]);

  return (
    <header className="mb-5 flex flex-col gap-2">
      {breadcrumbs.length > 0 && (
        <nav aria-label="Breadcrumb">
          <ol className="flex flex-wrap items-center gap-1 text-[13px] text-muted-foreground">
            {breadcrumbs.map((c, i) => {
              const last = i === breadcrumbs.length - 1;
              return (
                <li key={`${c.label}-${i}`} className="flex items-center gap-1">
                  {c.to && !last ? (
                    <Link to={c.to} className="rounded hover:text-fg hover:underline">
                      {c.label}
                    </Link>
                  ) : (
                    <span aria-current={last ? "page" : undefined} className={last ? "text-fg" : undefined}>
                      {c.label}
                    </span>
                  )}
                  {!last && <Icon name="chevron" className="size-3 text-subtle" />}
                </li>
              );
            })}
          </ol>
        </nav>
      )}
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div className="min-w-0">
          <h1 className="truncate text-xl font-semibold tracking-tight">{title}</h1>
          {description && <div className="mt-1 text-[13px] text-muted-foreground">{description}</div>}
        </div>
        {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
      </div>
    </header>
  );
}
