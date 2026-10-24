import type { ReactNode } from "react";
import { Navigate, Outlet, useLocation } from "react-router";
import { Spinner } from "@/components/ui/Spinner";
import { ErrorState } from "@/components/ui/States";
import { Wordmark } from "@/components/Wordmark";
import { useAuth } from "./context";

/** Gate for authenticated routes: redirects to /login without a session. */
export function RequireAuth() {
  const { token, me } = useAuth();
  const location = useLocation();

  if (!token) {
    return <Navigate to="/login" replace state={{ from: `${location.pathname}${location.search}` }} />;
  }
  if (me.isPending) {
    return (
      <div role="status" className="flex h-full flex-col items-center justify-center gap-3 text-muted">
        <Wordmark className="text-lg" />
        <span className="flex items-center gap-2 text-sm">
          <Spinner /> Loading session…
        </span>
      </div>
    );
  }
  if (me.isError) {
    return (
      <div className="flex h-full items-center justify-center">
        <ErrorState error={me.error} onRetry={() => void me.refetch()} />
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
      <div role="alert" className="flex flex-col items-center gap-2 py-16 text-center">
        <p className="font-medium">Server admin access required</p>
        <p className="text-[13px] text-muted">Ask a server administrator to grant your account admin rights.</p>
      </div>
    );
  }
  return <>{children}</>;
}
