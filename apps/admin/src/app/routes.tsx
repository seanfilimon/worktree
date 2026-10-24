import { Navigate, type RouteObject } from "react-router";
import { RequireAuth } from "@/auth/RequireAuth";
import { AppLayout } from "@/components/AppLayout";
import { AccountsPage } from "@/pages/AccountsPage";
import { AuditPage } from "@/pages/AuditPage";
import { LoginPage } from "@/pages/auth/LoginPage";
import { RegisterPage } from "@/pages/auth/RegisterPage";
import { DashboardPage } from "@/pages/DashboardPage";
import { NotFoundPage } from "@/pages/NotFoundPage";
import { SettingsPage } from "@/pages/SettingsPage";
import { TenantDetailPage } from "@/pages/tenants/TenantDetailPage";
import { TenantsPage } from "@/pages/tenants/TenantsPage";
import { BranchesTab } from "@/pages/tree/BranchesTab";
import { FilesTab } from "@/pages/tree/FilesTab";
import { HistoryTab } from "@/pages/tree/HistoryTab";
import { LicensesTab } from "@/pages/tree/LicensesTab";
import { MergeRequestDetail } from "@/pages/tree/MergeRequestDetail";
import { MergeRequestsTab } from "@/pages/tree/MergeRequestsTab";
import { ProtectionsTab } from "@/pages/tree/ProtectionsTab";
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
              { index: true, element: <Navigate to="branches" replace /> },
              { path: "branches", element: <BranchesTab /> },
              { path: "history", element: <HistoryTab /> },
              { path: "files", element: <FilesTab /> },
              { path: "staged", element: <StagedTab /> },
              { path: "tags", element: <TagsTab /> },
              { path: "merge-requests", element: <MergeRequestsTab /> },
              { path: "merge-requests/:number", element: <MergeRequestDetail /> },
              { path: "protections", element: <ProtectionsTab /> },
              { path: "licenses", element: <LicensesTab /> },
              { path: "settings", element: <TreeSettingsTab /> },
            ],
          },
          { path: "audit", element: <AuditPage /> },
          { path: "accounts", element: <AccountsPage /> },
          { path: "settings", element: <SettingsPage /> },
          { path: "*", element: <NotFoundPage /> },
        ],
      },
    ],
  },
];
