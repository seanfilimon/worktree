import { CircleAlert, Loader2, ShieldAlert } from "lucide-react";
import type { ReactNode } from "react";
import { Navigate, Outlet, useLocation } from "react-router";
import { errorMessage } from "@/api/client";
import { Wordmark } from "@/components/Wordmark";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { useAuth } from "./context";

/** Gate for authenticated routes: redirects to /login without a session. */
export function RequireAuth() {
  const { token, me, signOut } = useAuth();
  const location = useLocation();

  if (!token) {
    return <Navigate to="/login" replace state={{ from: `${location.pathname}${location.search}` }} />;
  }
  if (me.isPending) {
    return (
      <div role="status" className="flex min-h-svh flex-col items-center justify-center gap-3 text-muted-foreground">
        <Wordmark className="text-lg" />
        <span className="flex items-center gap-2 text-sm">
          <Loader2 aria-hidden="true" className="size-4 animate-spin motion-reduce:animate-none" />
          Loading session…
        </span>
      </div>
    );
  }
  if (me.isError) {
    return (
      <div className="flex min-h-svh items-center justify-center px-4">
        <Alert className="max-w-md">
          <CircleAlert aria-hidden="true" />
          <AlertTitle>Could not load your session</AlertTitle>
          <AlertDescription>
            <p>{errorMessage(me.error)}</p>
            <div className="mt-2 flex gap-2">
              <Button size="sm" onClick={() => void me.refetch()}>
                Retry
              </Button>
              <Button size="sm" variant="outline" onClick={() => void signOut()}>
                Sign out
              </Button>
            </div>
          </AlertDescription>
        </Alert>
      </div>
    );
  }
  return <Outlet />;
}

/** Wraps server-admin-only pages. */
export function RequireAdmin({ children }: { children: ReactNode }) {
  const { isAdmin } = useAuth();
  if (!isAdmin) {
    return (
      <div
        role="alert"
        className="flex flex-col items-center gap-2 rounded-lg border border-dashed px-6 py-16 text-center"
      >
        <ShieldAlert aria-hidden="true" className="size-6 text-muted-foreground" />
        <p className="font-medium">Server admin access required</p>
        <p className="max-w-sm text-sm text-muted-foreground">
          Ask a server administrator to grant your account admin rights.
        </p>
      </div>
    );
  }
  return <>{children}</>;
}
