import { Navigate, type RouteObject } from "react-router";
import { RequireAuth } from "@/auth/RequireAuth";
import { AppLayout } from "@/components/AppLayout";
import { AccountsPage } from "@/pages/AccountsPage";
import { AuditPage } from "@/pages/AuditPage";
import { LoginPage } from "@/pages/auth/LoginPage";
import { RegisterPage } from "@/pages/auth/RegisterPage";
import { DashboardPage } from "@/pages/DashboardPage";
import { AccessSimulatorPage } from "@/pages/iam/AccessSimulatorPage";
import { TreeAccessTab } from "@/pages/iam/TreeAccessTab";
import { NotFoundPage } from "@/pages/NotFoundPage";
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
import { ProtectionsTab } from "@/pages/tree/ProtectionsTab";
import { SnapshotDetail } from "@/pages/tree/SnapshotDetail";
import { StagedTab } from "@/pages/tree/StagedTab";
import { TagsTab } from "@/pages/tree/TagsTab";
import { TreeLayout } from "@/pages/tree/TreeLayout";
import { TreeSettingsTab } from "@/pages/tree/TreeSettingsTab";
import { TreesPage } from "@/pages/trees/TreesPage";

export const routes: RouteObject[] = [
  { path: "/login", element: <LoginPage /> },
  { path: "/register", element: <RegisterPage /> },
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
              { path: "merge-requests", element: <MergeRequestsTab /> },
              { path: "merge-requests/:number", element: <MergeRequestDetail /> },
              { path: "protections", element: <ProtectionsTab /> },
              { path: "licenses", element: <LicensesTab /> },
              { path: "access", element: <TreeAccessTab /> },
              { path: "settings", element: <TreeSettingsTab /> },
            ],
          },
          { path: "audit", element: <AuditPage /> },
          { path: "iam/simulator", element: <AccessSimulatorPage /> },
          { path: "accounts", element: <AccountsPage /> },
          { path: "system", element: <SystemPage /> },
          { path: "settings", element: <SettingsPage /> },
          { path: "*", element: <NotFoundPage /> },
        ],
      },
    ],
  },
];
