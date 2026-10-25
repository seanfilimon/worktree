import { Fragment } from "react";
import { Link } from "react-router";
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from "@/components/ui/breadcrumb";
import { cn } from "@/lib/utils";
import type { Crumb } from "./headerTrail";

interface BreadcrumbsProps {
  crumbs: Crumb[];
  /** Show only the current page below `md` (the header bar is narrow). */
  collapseOnMobile?: boolean;
  className?: string;
}

/** shadcn breadcrumb trail; the last crumb is the current page. */
export function Breadcrumbs({ crumbs, collapseOnMobile = false, className }: BreadcrumbsProps) {
  if (crumbs.length === 0) return null;
  const hideOnMobile = collapseOnMobile ? "hidden md:inline-flex" : undefined;

  return (
    <Breadcrumb aria-label="Breadcrumb" className={cn("min-w-0", className)}>
      <BreadcrumbList className="flex-nowrap">
        {crumbs.map((c, i) => {
          const last = i === crumbs.length - 1;
          return (
            <Fragment key={`${c.label}-${i}`}>
              <BreadcrumbItem className={cn(last ? "min-w-0" : hideOnMobile)}>
                {last || !c.to ? (
                  // A plain label rather than shadcn's disabled role="link".
                  <BreadcrumbPage
                    role={undefined}
                    aria-disabled={undefined}
                    aria-current={last ? "page" : undefined}
                    className={cn("truncate", !last && "text-muted-foreground")}
                  >
                    {c.label}
                  </BreadcrumbPage>
                ) : (
                  <BreadcrumbLink asChild>
                    <Link to={c.to}>{c.label}</Link>
                  </BreadcrumbLink>
                )}
              </BreadcrumbItem>
              {!last && <BreadcrumbSeparator className={hideOnMobile} />}
            </Fragment>
          );
        })}
      </BreadcrumbList>
    </Breadcrumb>
  );
}
