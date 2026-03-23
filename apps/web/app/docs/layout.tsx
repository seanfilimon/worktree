import { DocsLayout } from "fumadocs-ui/layouts/docs";
import type { ReactNode } from "react";
import { docsSource } from "@/lib/source";
import { baseOptions } from "@/lib/layout.shared";
import { VersionSelector } from "@/components/version-selector";

const versions = [
  {
    name: "v0.1.0-alpha",
    url: "/docs",
    active: true,
    label: "latest",
  },
  {
    name: "v0.0.1-dev",
    url: "/docs",
    label: "deprecated",
  },
];

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <DocsLayout
      tree={docsSource.pageTree}
      {...baseOptions()}
      sidebar={{
        banner: <VersionSelector versions={versions} />,
      }}
    >
      {children}
    </DocsLayout>
  );
}
