import { Navigate, type RouteObject } from "react-router";
import { RequireAuth } from "@/auth/RequireAuth";
import { AppLayout } from "@/components/AppLayout";
import { AccountsPage } from "@/pages/AccountsPage";
import { AuditPage } from "@/pages/AuditPage";
import { SsoSettingsPage } from "@/pages/admin/SsoSettingsPage";
import { InvitePage } from "@/pages/auth/InvitePage";
import { LoginPage } from "@/pages/auth/LoginPage";
import { RegisterPage } from "@/pages/auth/RegisterPage";
import { ResetPasswordPage } from "@/pages/auth/ResetPasswordPage";
import { DashboardPage } from "@/pages/DashboardPage";
import { AccessSimulatorPage } from "@/pages/iam/AccessSimulatorPage";
import { TreeAccessTab } from "@/pages/iam/TreeAccessTab";
import { NotFoundPage } from "@/pages/NotFoundPage";
import { OperationsPage } from "@/pages/OperationsPage";
import { BackupsPage } from "@/pages/BackupsPage";
import { CompliancePage } from "@/pages/CompliancePage";
import { DevicePage } from "@/pages/DevicePage";
import { ComplianceTab } from "@/pages/tree/ComplianceTab";
import { ForksTab } from "@/pages/tree/ForksTab";
import { ResolveConflictsPage } from "@/pages/tree/ResolveConflictsPage";
import { TreeConfigTab } from "@/pages/tree/TreeConfigTab";
import { NotificationsPage } from "@/pages/notifications/NotificationsPage";
import { MirrorsTab } from "@/pages/tree/MirrorsTab";
import { OwnersTab } from "@/pages/tree/OwnersTab";
import { CodeSearchPage } from "@/pages/search/CodeSearchPage";
import { SettingsPage } from "@/pages/SettingsPage";
import { SystemPage } from "@/pages/SystemPage";
import { TenantDetailPage } from "@/pages/tenants/TenantDetailPage";
import { TenantsPage } from "@/pages/tenants/TenantsPage";
import { BranchesTab } from "@/pages/tree/BranchesTab";
import { CompareView } from "@/pages/tree/CompareView";
import { FilesTab } from "@/pages/tree/FilesTab";
import { HistoryTab } from "@/pages/tree/HistoryTab";
import { LicensesTab } from "@/pages/tree/LicensesTab";
import { MergeRequestDetail } from "@/pages/tree/MergeRequestDetail";
import { MergeRequestsTab } from "@/pages/tree/MergeRequestsTab";
import { OverviewTab } from "@/pages/tree/OverviewTab";
import { ReleasesTab } from "@/pages/tree/ReleasesTab";
import { ProtectionsTab } from "@/pages/tree/ProtectionsTab";
import { SnapshotDetail } from "@/pages/tree/SnapshotDetail";
import { StagedTab } from "@/pages/tree/StagedTab";
import { TagsTab } from "@/pages/tree/TagsTab";
import { TodosTab } from "@/pages/tree/TodosTab";
import { TreeLayout } from "@/pages/tree/TreeLayout";
import { TreeSettingsTab } from "@/pages/tree/TreeSettingsTab";
import { TreesPage } from "@/pages/trees/TreesPage";

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
          { path: "tenants", element: <TenantsPage /> },
          { path: "tenants/:tenant", element: <TenantDetailPage /> },
          { path: "trees", element: <TreesPage /> },
          {
            path: "trees/:tenant/:tree",
            element: <TreeLayout />,
            children: [
              { index: true, element: <Navigate to="overview" replace /> },
              { path: "overview", element: <OverviewTab /> },
              { path: "branches", element: <BranchesTab /> },
              { path: "history", element: <HistoryTab /> },
              { path: "snapshots/:id", element: <SnapshotDetail /> },
              { path: "compare", element: <CompareView /> },
              { path: "files", element: <FilesTab /> },
              { path: "staged", element: <StagedTab /> },
              { path: "tags", element: <TagsTab /> },
              { path: "releases", element: <ReleasesTab /> },
              { path: "todos", element: <TodosTab /> },
              { path: "merge-requests", element: <MergeRequestsTab /> },
              { path: "merge-requests/:number", element: <MergeRequestDetail /> },
              { path: "merge-requests/:number/resolve", element: <ResolveConflictsPage /> },
              { path: "protections", element: <ProtectionsTab /> },
              { path: "licenses", element: <LicensesTab /> },
              { path: "access", element: <TreeAccessTab /> },
              { path: "owners", element: <OwnersTab /> },
              { path: "mirrors", element: <MirrorsTab /> },
              { path: "forks", element: <ForksTab /> },
              { path: "config", element: <TreeConfigTab /> },
              { path: "compliance", element: <ComplianceTab /> },
              { path: "settings", element: <TreeSettingsTab /> },
            ],
          },
          { path: "search/code", element: <CodeSearchPage /> },
          { path: "audit", element: <AuditPage /> },
          { path: "iam/simulator", element: <AccessSimulatorPage /> },
          { path: "accounts", element: <AccountsPage /> },
          { path: "system", element: <SystemPage /> },
          { path: "operations", element: <OperationsPage /> },
          { path: "backups", element: <BackupsPage /> },
          { path: "notifications", element: <NotificationsPage /> },
          { path: "device", element: <DevicePage /> },
          { path: "compliance", element: <CompliancePage /> },
          { path: "sso", element: <SsoSettingsPage /> },
          { path: "settings", element: <SettingsPage /> },
          { path: "*", element: <NotFoundPage /> },
        ],
      },
    ],
  },
];
