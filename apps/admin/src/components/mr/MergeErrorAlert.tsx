import { CircleX, ShieldAlert, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import type { MergeErrorInfo } from "@/lib/mergeErrors";

const ICON = {
  review_required: ShieldAlert,
  self_approval: ShieldAlert,
  forbidden: ShieldAlert,
  diverged: TriangleAlert,
  target_moved: TriangleAlert,
  already_open: TriangleAlert,
  not_open: TriangleAlert,
  invalid: CircleX,
  other: CircleX,
} satisfies Record<MergeErrorInfo["kind"], unknown>;

/** A classified merge request error as a monochrome alert (border weight, icon, label). */
export function MergeErrorAlert({ error, children }: { error: MergeErrorInfo; children?: ReactNode }) {
  const Icon = ICON[error.kind];
  return (
    <Alert data-kind={error.kind} className="border-foreground/60">
      <Icon aria-hidden />
      <AlertTitle className="line-clamp-none">{error.title}</AlertTitle>
      <AlertDescription>
        <p>{error.message}</p>
        {children}
        {error.detail !== error.message && <p className="font-mono text-xs break-all">Server: {error.detail}</p>}
      </AlertDescription>
    </Alert>
  );
}
