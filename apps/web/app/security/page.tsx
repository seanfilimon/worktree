import * as React from "react";
import { SiteHeader } from "@/components/site-header";

export default function SecurityPage() {
  return (
    <div className="flex min-h-screen flex-col bg-background">
      <SiteHeader />
      <main className="flex-1 container mx-auto px-4 py-20 max-w-4xl">
        <h1 className="text-4xl font-bold mb-6">Security Architecture</h1>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-6 mt-8">
          <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold">Transport Security</h3><p className="text-sm text-muted-foreground mt-2">TLS 1.3 everywhere, QUIC primary</p></div>
          <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold">Authentication</h3><p className="text-sm text-muted-foreground mt-2">OAuth2 device flow, API keys, JWT</p></div>
          <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold">Secret Scanning</h3><p className="text-sm text-muted-foreground mt-2">Pre-snapshot, configurable patterns</p></div>
          <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold">Snapshot Signing</h3><p className="text-sm text-muted-foreground mt-2">Ed25519</p></div>
          <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold">Data Encryption</h3><p className="text-sm text-muted-foreground mt-2">In-transit + at-rest</p></div>
          <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold">IPC Security</h3><p className="text-sm text-muted-foreground mt-2">Unix socket / named pipe, owner-only</p></div>
          <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold">Threat Model</h3><p className="text-sm text-muted-foreground mt-2">12 threats covered and mitigated</p></div>
          <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold">Rate Limiting</h3><p className="text-sm text-muted-foreground mt-2">Per plan enforcement</p></div>
          <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold">Incident Response</h3><p className="text-sm text-muted-foreground mt-2">Automated alerting, token revocation</p></div>
        </div>
      </main>
    </div>
  );
}
