import { ArrowRight, Signpost } from "lucide-react";
import { Link } from "react-router";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";

/** Shown for a renamed tenant's old slug (`301 tenant_moved`, §16.4). */
export function TenantMovedNotice({ from, to }: { from: string; to: string }) {
  return (
    <div className="p-4">
      <Alert>
        <Signpost aria-hidden="true" />
        <AlertTitle>This tenant was renamed</AlertTitle>
        <AlertDescription>
          <p>
            <span className="font-mono">{from}</span> is now <span className="font-mono text-foreground">{to}</span>. The old slug stays
            reserved for 90 days. Update worktree remotes with <span className="font-mono text-foreground">wt tenant switch {to}</span>.
          </p>
          <Button asChild size="sm" className="mt-1">
            <Link to={`/tenants/${encodeURIComponent(to)}`}>
              Go to {to}
              <ArrowRight aria-hidden="true" />
            </Link>
          </Button>
        </AlertDescription>
      </Alert>
    </div>
  );
}
