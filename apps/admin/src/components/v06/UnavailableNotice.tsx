import { Construction } from "lucide-react";
import type { ReactNode } from "react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { cn } from "@/lib/utils";

/** Shown when a v0.6 backend is not deployed (501, or the API's 404 "no such endpoint"). */
export function UnavailableNotice({ feature, className, children }: { feature: string; className?: string; children?: ReactNode }) {
  return (
    <Alert className={cn("w-auto", className)} role="status">
      <Construction aria-hidden="true" />
      <AlertTitle>{feature} is not available on this server</AlertTitle>
      <AlertDescription>{children ?? "This W0rkTree server does not provide it yet. Upgrade the server to use it."}</AlertDescription>
    </Alert>
  );
}
