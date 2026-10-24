import * as React from "react";
import { SiteHeader } from "@/components/site-header";

export default function AboutPage() {
  return (
    <div className="flex min-h-screen flex-col bg-background">
      <SiteHeader />
      <main className="flex-1 container mx-auto px-4 py-20 max-w-4xl">
        <h1 className="text-4xl font-bold mb-6">About W0rkTree</h1>
        <p className="text-lg text-muted-foreground mb-8">
          W0rkTree is not a Git wrapper. It replaces Git at the protocol level. We built W0rkTree to solve the inherent
          collaboration and security issues in legacy version control systems.
        </p>
        <h2 className="text-2xl font-semibold mb-4">Technical Philosophy</h2>
        <ul className="list-disc pl-6 space-y-2 mb-8 text-muted-foreground">
          <li><strong>Two-runtime architecture:</strong> Local BGProcess + Remote Server.</li>
          <li><strong>Staged visibility:</strong> See team activity in real-time.</li>
          <li><strong>Multi-tenancy:</strong> Built-in IAM.</li>
          <li><strong>Declarative access:</strong> TOML-based RBAC.</li>
          <li><strong>License compliance:</strong> Protocol-level SPDX enforcement.</li>
        </ul>
      </main>
    </div>
  );
}
