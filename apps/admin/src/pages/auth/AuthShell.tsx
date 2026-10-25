import { useEffect, type ReactNode } from "react";
import { useHealth } from "@/api/queries";
import { Wordmark } from "@/components/Wordmark";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { cn } from "@/lib/utils";

/** Server reachability and version, from GET /healthz. */
function ServerStatus() {
  const health = useHealth();
  const label = health.isPending
    ? "Contacting server…"
    : health.isError
      ? "Server unreachable"
      : `Server ${health.data.version}`;
  return (
    <p role="status" className="flex items-center justify-center gap-2 text-xs text-muted-foreground">
      <span
        aria-hidden="true"
        className={cn(
          "inline-block size-2 rounded-full border border-foreground",
          health.isSuccess && "bg-foreground",
          health.isPending && "animate-pulse motion-reduce:animate-none",
          health.isError && "border-muted-foreground",
        )}
      />
      <span className="font-mono">{label}</span>
    </p>
  );
}

interface AuthShellProps {
  title: string;
  description: string;
  children: ReactNode;
  /** Below the form, inside the card (e.g. a link to the other auth page). */
  footer?: ReactNode;
}

/** Centered card used by the sign-in and registration pages. */
export function AuthShell({ title, description, children, footer }: AuthShellProps) {
  useEffect(() => {
    document.title = `${title} · W0rkTree Admin`;
  }, [title]);

  return (
    <main className="flex min-h-svh flex-col items-center justify-center gap-6 bg-muted/40 px-4 py-12">
      <div className="flex flex-col items-center gap-1 text-center">
        <Wordmark className="text-2xl" />
        <p className="text-sm text-muted-foreground">Server administration</p>
      </div>
      <Card className="w-full max-w-sm gap-5">
        <CardHeader>
          <CardTitle>
            <h1 className="text-lg font-semibold tracking-tight">{title}</h1>
          </CardTitle>
          <CardDescription>{description}</CardDescription>
        </CardHeader>
        <CardContent>{children}</CardContent>
        {footer && (
          <CardFooter className="justify-center border-t text-sm text-muted-foreground [.border-t]:pt-5">
            {footer}
          </CardFooter>
        )}
      </Card>
      <ServerStatus />
    </main>
  );
}
