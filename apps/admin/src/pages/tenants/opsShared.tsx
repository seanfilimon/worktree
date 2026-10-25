import { Ban, CircleCheck, Construction, PauseCircle } from "lucide-react";
import type { ReactNode } from "react";
import type { TenantStatus } from "@/api/v05ops";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

/** Shown when the server answers `501 not_implemented` for a v0.5 feature. */
export function NotImplementedNotice({ feature, className, children }: { feature: string; className?: string; children?: ReactNode }) {
  return (
    <Alert className={cn("w-auto", className)} role="status">
      <Construction aria-hidden="true" />
      <AlertTitle>{feature} is not available on this server</AlertTitle>
      <AlertDescription>
        {children ?? "The server answered 501 not implemented. Upgrade the W0rkTree server to use this feature."}
      </AlertDescription>
    </Alert>
  );
}

const STATUS_LABEL: Record<TenantStatus, string> = {
  active: "Active",
  suspended: "Suspended",
  deactivated: "Deactivated",
};

/** Active = outline; suspended = filled; deactivated = filled and struck through. */
export function TenantStatusBadge({ status }: { status: TenantStatus }) {
  if (status === "active") {
    return (
      <Badge variant="outline">
        <CircleCheck aria-hidden="true" />
        {STATUS_LABEL.active}
      </Badge>
    );
  }
  return (
    <Badge>
      {status === "suspended" ? <PauseCircle aria-hidden="true" /> : <Ban aria-hidden="true" />}
      <span className={cn(status === "deactivated" && "line-through")}>{STATUS_LABEL[status]}</span>
    </Badge>
  );
}
