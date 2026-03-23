import { SiteHeader } from "@/components/site-header";
import type { ReactNode } from "react";

export default function ArticlesLayout({ children }: { children: ReactNode }) {
  return (
    <div className="flex min-h-screen flex-col bg-background">
      <SiteHeader />
      <main className="flex-1">{children}</main>
    </div>
  );
}
