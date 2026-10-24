import { useEffect, type ReactNode } from "react";
import { Wordmark } from "@/components/Wordmark";

/** Centered card used by the sign-in and registration pages. */
export function AuthShell({ title, children, footer }: { title: string; children: ReactNode; footer?: ReactNode }) {
  useEffect(() => {
    document.title = `${title} · W0rkTree Admin`;
  }, [title]);

  return (
    <main className="flex min-h-full items-center justify-center px-4 py-12">
      <div className="w-full max-w-sm">
        <div className="mb-6 text-center">
          <Wordmark className="text-2xl" />
          <p className="mt-1 text-[13px] text-muted-foreground">Server administration</p>
        </div>
        <div className="rounded-lg border border-border bg-surface p-6 shadow-card">
          <h1 className="mb-4 text-base font-semibold">{title}</h1>
          {children}
        </div>
        {footer && <div className="mt-4 text-center text-[13px] text-muted-foreground">{footer}</div>}
      </div>
    </main>
  );
}
