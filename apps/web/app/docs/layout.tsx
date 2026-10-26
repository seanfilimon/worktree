import { DocsLayout } from "fumadocs-ui/layouts/docs";
import type { ReactNode } from "react";
import { SidebarFooter } from "@/components/sidebar-footer";
import { baseOptions } from "@/lib/layout.shared";
import { docsSource } from "@/lib/source";

export default function Layout({ children }: { children: ReactNode }) {
  const base = baseOptions();
  return (
    <DocsLayout
      tree={docsSource.pageTree}
      {...base}
      links={base.links?.filter((link) => link.type === "icon")}
      sidebar={{ footer: <SidebarFooter /> }}
    >
      {children}
    </DocsLayout>
  );
}
