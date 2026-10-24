import { DocsLayout } from "fumadocs-ui/layouts/docs";
import type { ReactNode } from "react";
import { docsSource } from "@/lib/source";
import { baseOptions } from "@/lib/layout.shared";
import { CURRENT_VERSION } from "@/lib/site";
import { VersionSelector, type Version } from "@/components/version-selector";

const versions: Version[] = [
  {
    name: CURRENT_VERSION,
    url: "/docs",
    active: true,
    label: "latest",
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
