import { useState } from "react";
import { NavLink, Outlet } from "react-router";
import { useAuth } from "@/auth/context";
import { cn } from "@/lib/cn";
import { Icon, type IconName } from "./icons";
import { Badge } from "./ui/Badge";
import { Wordmark } from "./Wordmark";

interface NavItem {
  to: string;
  label: string;
  icon: IconName;
  end?: boolean;
  adminOnly?: boolean;
}

const NAV: NavItem[] = [
  { to: "/", label: "Dashboard", icon: "dashboard", end: true },
  { to: "/tenants", label: "Tenants", icon: "tenants" },
  { to: "/trees", label: "Trees", icon: "trees" },
  { to: "/audit", label: "Audit log", icon: "audit" },
  { to: "/accounts", label: "Accounts", icon: "accounts", adminOnly: true },
  { to: "/settings", label: "Settings", icon: "settings" },
];

function Sidebar({ onNavigate }: { onNavigate: () => void }) {
  const { account, isAdmin, signOut } = useAuth();
  return (
    <div className="flex h-full flex-col">
      <div className="flex h-14 items-center border-b border-border px-4">
        <Wordmark className="text-base" suffix="admin" />
      </div>
      <nav aria-label="Main" className="flex-1 overflow-y-auto p-2">
        <ul className="flex flex-col gap-0.5">
          {NAV.filter((n) => !n.adminOnly || isAdmin).map((n) => (
            <li key={n.to}>
              <NavLink
                to={n.to}
                end={n.end}
                onClick={onNavigate}
                className={({ isActive }) =>
                  cn(
                    "flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-sm font-medium transition-colors",
                    isActive ? "bg-accent-soft text-accent" : "text-muted hover:bg-surface-2 hover:text-fg",
                  )
                }
              >
                <Icon name={n.icon} />
                {n.label}
              </NavLink>
            </li>
          ))}
        </ul>
      </nav>
      <div className="flex items-center gap-2 border-t border-border p-3">
        <div aria-hidden="true" className="flex size-8 items-center justify-center rounded-full bg-surface-2 font-mono text-xs font-semibold uppercase">
          {account?.username.slice(0, 2)}
        </div>
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium">{account?.display_name || account?.username}</p>
          <p className="flex items-center gap-1 truncate text-xs text-muted">
            @{account?.username}
            {isAdmin && <Badge tone="accent">admin</Badge>}
          </p>
        </div>
        <button
          type="button"
          onClick={() => void signOut()}
          className="rounded-md p-1.5 text-muted hover:bg-surface-2 hover:text-fg"
          aria-label="Sign out"
          title="Sign out"
        >
          <Icon name="logout" />
        </button>
      </div>
    </div>
  );
}

export function AppLayout() {
  const [open, setOpen] = useState(false);
  const close = () => setOpen(false);

  return (
    <div className="flex h-full">
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-50 focus:rounded focus:bg-surface focus:px-3 focus:py-2"
      >
        Skip to content
      </a>

      <aside className="hidden w-60 shrink-0 border-r border-border bg-surface md:block">
        <Sidebar onNavigate={close} />
      </aside>

      {open && (
        <div
          className="fixed inset-0 z-40 md:hidden"
          role="dialog"
          aria-modal="true"
          aria-label="Navigation"
          onKeyDown={(e) => {
            if (e.key === "Escape") close();
          }}
        >
          <button type="button" aria-label="Close navigation" className="absolute inset-0 bg-black/40" onClick={close} />
          <div className="absolute inset-y-0 left-0 w-64 border-r border-border bg-surface shadow-card">
            <Sidebar onNavigate={close} />
          </div>
        </div>
      )}

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 items-center gap-3 border-b border-border bg-surface px-4 md:hidden">
          <button
            type="button"
            onClick={() => setOpen(true)}
            aria-label="Open navigation"
            aria-expanded={open}
            className="rounded-md p-1.5 text-muted hover:bg-surface-2 hover:text-fg"
          >
            <Icon name="menu" />
          </button>
          <Wordmark suffix="admin" />
        </header>
        <main id="main" tabIndex={-1} className="flex-1 overflow-y-auto focus:outline-none">
          <div className="mx-auto w-full max-w-7xl px-4 py-6 sm:px-6">
            <Outlet />
          </div>
        </main>
      </div>
    </div>
  );
}
