import { ChevronRight } from "lucide-react";
import { Fragment, useState } from "react";
import type { AuditEvent } from "@/api/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { absoluteTime } from "@/lib/format";
import { cn } from "@/lib/utils";
import { TimeAgo } from "./TimeAgo";

/** Filled = needs attention, secondary = authentication, outline = everything else. */
function actionVariant(action: string): "default" | "secondary" | "outline" {
  if (/(\.delete|\.remove|_failed|_rejected|\.denied)$/.test(action)) return "default";
  if (action.startsWith("auth.")) return "secondary";
  return "outline";
}

interface AuditTableProps {
  events: readonly AuditEvent[];
  /** Tenant slug for an id, when known. */
  tenantSlug: (id: string) => string | undefined;
}

const COLUMNS = ["When", "Actor", "Action", "Resource", "Tenant", "Source"];

/** Audit events with expandable JSON details and a sticky header. */
export function AuditTable({ events, tenantSlug }: AuditTableProps) {
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());
  const toggle = (id: string) =>
    setOpen((s) => {
      const next = new Set(s);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  return (
    <div className="max-h-[70vh] overflow-auto">
      <table aria-label="Audit events" className="w-full caption-bottom text-sm">
        <TableHeader className="sticky top-0 z-10 bg-card shadow-[inset_0_-1px_0_var(--border)]">
          <TableRow className="hover:bg-transparent">
            <TableHead className="w-10 pl-4">
              <span className="sr-only">Details</span>
            </TableHead>
            {COLUMNS.map((c) => (
              <TableHead key={c}>{c}</TableHead>
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          {events.map((e) => {
            const expanded = open.has(e.id);
            const detailsId = `audit-details-${e.id}`;
            const slug = tenantSlug(e.tenant_id);
            const details = Object.entries(e.details ?? {});
            return (
              <Fragment key={e.id}>
                <TableRow data-state={expanded ? "selected" : undefined}>
                  <TableCell className="pl-4">
                    <Button
                      size="icon-xs"
                      variant="ghost"
                      aria-expanded={expanded}
                      aria-controls={expanded ? detailsId : undefined}
                      aria-label={`Details: ${e.action} by ${e.actor}`}
                      onClick={() => toggle(e.id)}
                    >
                      <ChevronRight aria-hidden="true" className={cn("transition-transform", expanded && "rotate-90")} />
                    </Button>
                  </TableCell>
                  <TableCell>
                    <TimeAgo value={e.at} className="text-muted-foreground" />
                  </TableCell>
                  <TableCell className="font-medium">{e.actor}</TableCell>
                  <TableCell>
                    <Badge variant={actionVariant(e.action)} className="font-mono">
                      {e.action}
                    </Badge>
                  </TableCell>
                  <TableCell className="max-w-72 truncate font-mono text-xs">
                    <span className="text-muted-foreground">{e.resource_type}</span>
                    {e.resource_id && <> {e.resource_id}</>}
                    {details.length > 0 && (
                      <span className="ml-2 text-muted-foreground">
                        {details.length} {details.length === 1 ? "detail" : "details"}
                      </span>
                    )}
                  </TableCell>
                  <TableCell className="font-mono text-xs">{slug ?? <span className="text-muted-foreground">{e.tenant_id || "—"}</span>}</TableCell>
                  <TableCell className="font-mono text-xs text-muted-foreground">{e.remote_addr || "—"}</TableCell>
                </TableRow>
                {expanded && (
                  <TableRow id={detailsId} className="bg-muted/30 hover:bg-muted/30">
                    <TableCell colSpan={COLUMNS.length + 1} className="px-4 py-3 whitespace-normal">
                      <p className="mb-2 text-xs text-muted-foreground">
                        <time dateTime={e.at}>{absoluteTime(e.at)}</time> · <span className="font-mono">{e.at}</span>
                      </p>
                      <pre className="overflow-x-auto rounded-md border bg-background p-3 font-mono text-xs leading-relaxed">
                        {JSON.stringify(e, null, 2)}
                      </pre>
                    </TableCell>
                  </TableRow>
                )}
              </Fragment>
            );
          })}
        </TableBody>
      </table>
    </div>
  );
}
