import { CircleCheck, CircleX, LoaderCircle } from "lucide-react";
import { cn } from "@/lib/utils";
import type { ReadinessCheck } from "./model";

const STATUS_LABEL = { pass: "Passed", fail: "Failed", pending: "Pending" } as const;

/** Merge readiness: passing checks in full weight, failing and pending ones dimmed. */
export function ReadinessChecklist({ checks }: { checks: ReadinessCheck[] }) {
  return (
    <ul aria-label="Merge readiness" className="flex flex-col gap-2.5 text-sm">
      {checks.map((c) => {
        const Icon = c.status === "pass" ? CircleCheck : c.status === "fail" ? CircleX : LoaderCircle;
        return (
          <li key={c.id} data-check={c.id} data-status={c.status} className="flex items-start gap-2">
            <Icon
              aria-hidden
              className={cn(
                "mt-0.5 size-4 shrink-0",
                c.status === "pending" && "animate-spin text-muted-foreground",
                c.status === "fail" && "text-muted-foreground",
              )}
            />
            <div className="min-w-0">
              <p className={cn(c.status === "pass" ? "font-medium" : "text-muted-foreground")}>
                <span className="sr-only">{STATUS_LABEL[c.status]}: </span>
                {c.label}
              </p>
              {c.detail && c.status !== "pending" && <p className="text-xs text-muted-foreground">{c.detail}</p>}
            </div>
          </li>
        );
      })}
    </ul>
  );
}
