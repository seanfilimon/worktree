import * as React from "react";
import { SiteHeader } from "@/components/site-header";

export default function RoadmapPage() {
  return (
    <div className="flex min-h-screen flex-col bg-background">
      <SiteHeader />
      <main className="flex-1 container mx-auto px-4 py-20 max-w-4xl">
        <h1 className="text-4xl font-bold mb-6">Roadmap</h1>
        <div className="space-y-6">
          <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold text-xl mb-2">Phase 1: Foundations</h3><p className="text-muted-foreground">Staged Snapshot Visibility implementation, BGProcess extraction</p></div>
          <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold text-xl mb-2">Phase 2: Governance</h3><p className="text-muted-foreground">Multi-Tenancy & IAM (server-side), License Compliance (server-side)</p></div>
          <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold text-xl mb-2">Phase 3: Control</h3><p className="text-muted-foreground">Declarative Access Control (config parsing + enforcement)</p></div>
          <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold text-xl mb-2">Phase 4: Sync</h3><p className="text-muted-foreground">Sync Protocol (QUIC + gRPC)</p></div>
        </div>
      </main>
    </div>
  );
}
