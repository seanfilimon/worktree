import { Navigate, type RouteObject } from "react-router";
import type { ComponentType } from "react";
import { RequireAuth } from "@/auth/RequireAuth";
import { AppLayout } from "@/components/AppLayout";
import { InvitePage } from "@/pages/auth/InvitePage";
import { LoginPage } from "@/pages/auth/LoginPage";
import { RegisterPage } from "@/pages/auth/RegisterPage";
import { ResetPasswordPage } from "@/pages/auth/ResetPasswordPage";
import { DashboardPage } from "@/pages/DashboardPage";
import { NotFoundPage } from "@/pages/NotFoundPage";
import { TreeLayout } from "@/pages/tree/TreeLayout";

/**
 * Route-level code splitting: each page below has its own chunk, so the
 * initial bundle only carries the shell, the auth pages and the dashboard.
 * `preloadRoutes` fetches the rest once the app is idle (main.tsx), so
 * later navigations do not wait for a download.
 */
const loaders: Array<() => Promise<unknown>> = [];

function page<M extends Record<string, unknown>>(load: () => Promise<M>, name: keyof M & string) {
  loaders.push(load);
  return async () => ({ Component: (await load())[name] as ComponentType });
}

/** Load every lazily split page (resolves when all chunks are loaded). */
export function preloadRoutes(): Promise<unknown[]> {
  return Promise.all(loaders.map((load) => load()));
}

export const routes: RouteObject[] = [
  { path: "/login", element: <LoginPage /> },
  { path: "/register", element: <RegisterPage /> },
  // Public: reachable signed in or out (§12.5).
  { path: "/invite/:code", element: <InvitePage /> },
  { path: "/reset-password", element: <ResetPasswordPage /> },
  {
    element: <RequireAuth />,
    children: [
      {
        element: <AppLayout />,
        children: [
          { index: true, element: <DashboardPage /> },
          { path: "tenants", lazy: page(() => import("@/pages/tenants/TenantsPage"), "TenantsPage") },
          { path: "tenants/:tenant", lazy: page(() => import("@/pages/tenants/TenantDetailPage"), "TenantDetailPage") },
          { path: "trees", lazy: page(() => import("@/pages/trees/TreesPage"), "TreesPage") },
          {
            path: "trees/:tenant/:tree",
            element: <TreeLayout />,
            children: [
              { index: true, element: <Navigate to="overview" replace /> },
              { path: "overview", lazy: page(() => import("@/pages/tree/OverviewTab"), "OverviewTab") },
              { path: "branches", lazy: page(() => import("@/pages/tree/BranchesTab"), "BranchesTab") },
              { path: "history", lazy: page(() => import("@/pages/tree/HistoryTab"), "HistoryTab") },
              { path: "snapshots/:id", lazy: page(() => import("@/pages/tree/SnapshotDetail"), "SnapshotDetail") },
              { path: "compare", lazy: page(() => import("@/pages/tree/CompareView"), "CompareView") },
              { path: "files", lazy: page(() => import("@/pages/tree/FilesTab"), "FilesTab") },
              { path: "staged", lazy: page(() => import("@/pages/tree/StagedTab"), "StagedTab") },
              { path: "tags", lazy: page(() => import("@/pages/tree/TagsTab"), "TagsTab") },
              { path: "releases", lazy: page(() => import("@/pages/tree/ReleasesTab"), "ReleasesTab") },
              { path: "todos", lazy: page(() => import("@/pages/tree/TodosTab"), "TodosTab") },
              { path: "merge-requests", lazy: page(() => import("@/pages/tree/MergeRequestsTab"), "MergeRequestsTab") },
              { path: "merge-requests/:number", lazy: page(() => import("@/pages/tree/MergeRequestDetail"), "MergeRequestDetail") },
              { path: "merge-requests/:number/resolve", lazy: page(() => import("@/pages/tree/ResolveConflictsPage"), "ResolveConflictsPage") },
              { path: "protections", lazy: page(() => import("@/pages/tree/ProtectionsTab"), "ProtectionsTab") },
              { path: "licenses", lazy: page(() => import("@/pages/tree/LicensesTab"), "LicensesTab") },
              { path: "access", lazy: page(() => import("@/pages/iam/TreeAccessTab"), "TreeAccessTab") },
              { path: "owners", lazy: page(() => import("@/pages/tree/OwnersTab"), "OwnersTab") },
              { path: "mirrors", lazy: page(() => import("@/pages/tree/MirrorsTab"), "MirrorsTab") },
              { path: "forks", lazy: page(() => import("@/pages/tree/ForksTab"), "ForksTab") },
              { path: "config", lazy: page(() => import("@/pages/tree/TreeConfigTab"), "TreeConfigTab") },
              { path: "compliance", lazy: page(() => import("@/pages/tree/ComplianceTab"), "ComplianceTab") },
              { path: "settings", lazy: page(() => import("@/pages/tree/TreeSettingsTab"), "TreeSettingsTab") },
            ],
          },
          { path: "search/code", lazy: page(() => import("@/pages/search/CodeSearchPage"), "CodeSearchPage") },
          { path: "audit", lazy: page(() => import("@/pages/AuditPage"), "AuditPage") },
          { path: "iam/simulator", lazy: page(() => import("@/pages/iam/AccessSimulatorPage"), "AccessSimulatorPage") },
          { path: "accounts", lazy: page(() => import("@/pages/AccountsPage"), "AccountsPage") },
          { path: "system", lazy: page(() => import("@/pages/SystemPage"), "SystemPage") },
          { path: "operations", lazy: page(() => import("@/pages/OperationsPage"), "OperationsPage") },
          { path: "backups", lazy: page(() => import("@/pages/BackupsPage"), "BackupsPage") },
          { path: "notifications", lazy: page(() => import("@/pages/notifications/NotificationsPage"), "NotificationsPage") },
          { path: "device", lazy: page(() => import("@/pages/DevicePage"), "DevicePage") },
          { path: "compliance", lazy: page(() => import("@/pages/CompliancePage"), "CompliancePage") },
          { path: "sso", lazy: page(() => import("@/pages/admin/SsoSettingsPage"), "SsoSettingsPage") },
          { path: "settings", lazy: page(() => import("@/pages/SettingsPage"), "SettingsPage") },
          { path: "*", element: <NotFoundPage /> },
        ],
      },
    ],
  },
];
