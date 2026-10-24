import * as React from "react";
import { SiteHeader } from "@/components/site-header";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import Link from "next/link";
import {
  IoEye, IoTerminal, IoTime, IoLockClosed, IoGitBranch,
  IoGitNetwork, IoServer, IoWarning, IoPeople, IoFolder,
  IoShieldCheckmark, IoGitMerge, IoArrowForward
} from "react-icons/io5";

export default function FeaturesPage() {
  return (
    <div className="flex min-h-screen flex-col bg-background">
      <SiteHeader />
      <main className="flex-1">
        {/* Section 1: Hero */}
        <section className="border-b py-24 bg-muted/30">
          <div className="container mx-auto px-4 text-center max-w-4xl">
            <Badge variant="outline" className="mb-6">The W0rkTree Protocol</Badge>
            <h1 className="text-5xl font-bold tracking-tight mb-6">Version control, rebuilt from the protocol up.</h1>
            <p className="text-xl text-muted-foreground mb-10">Not a Git wrapper. Not a Git extension. A complete replacement with full Git compatibility as a migration bridge.</p>
            <div className="flex justify-center gap-4">
              <Button size="lg" asChild><Link href="/guides/quick-start">Get Started</Link></Button>
              <Button size="lg" variant="outline" asChild><Link href="/docs/protocol">Read the Specs</Link></Button>
            </div>
          </div>
        </section>

        {/* Section 2: Staged Snapshot Visibility */}
        <section id="staged-visibility" className="py-20 border-b">
          <div className="container mx-auto px-4 max-w-5xl">
            <Badge className="mb-4">Real-Time Collaboration</Badge>
            <h2 className="text-3xl font-bold mb-4">See what your team is working on — before they push.</h2>
            <p className="text-muted-foreground mb-10">Git provides zero in-flight visibility. Teams resort to standups and Slack to ask 'what are you working on?' W0rkTree answers this at the protocol level.</p>
            <div className="grid md:grid-cols-3 gap-6">
              <div className="p-6 bg-card border rounded-xl shadow-sm"><h3 className="font-bold mb-2">Live Activity Feed</h3><p className="text-sm text-muted-foreground">Staged snapshots sync automatically. See who's editing what, on which branch.</p></div>
              <div className="p-6 bg-card border rounded-xl shadow-sm"><h3 className="font-bold mb-2">Early Conflict Detection</h3><p className="text-sm text-muted-foreground">Advisory warnings before merge conflicts happen.</p></div>
              <div className="p-6 bg-card border rounded-xl shadow-sm"><h3 className="font-bold mb-2">Privacy Controls</h3><p className="text-sm text-muted-foreground">Opt out per tree, pause staging, private branches.</p></div>
            </div>
            <div className="mt-8 bg-muted p-4 rounded-lg font-mono text-sm"><code>wt status --team</code></div>
          </div>
        </section>

        {/* Section 3: Nested Trees */}
        <section id="trees" className="py-20 border-b bg-muted/10">
          <div className="container mx-auto px-4 max-w-5xl">
            <Badge className="mb-4" variant="outline">Core Protocol</Badge>
            <h2 className="text-3xl font-bold mb-10">Nested Trees</h2>
            <div className="grid md:grid-cols-3 gap-6">
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Tree Isolation</h3><p className="text-sm text-muted-foreground">Independent versioning, branches, access rules, licensing per tree.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Snapshots & History</h3><p className="text-sm text-muted-foreground">BLAKE3 content-addressed, immutable, append-only DAG.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Replaces Submodules</h3><p className="text-sm text-muted-foreground">No more git submodule pain. Trees are first-class.</p></div>
            </div>
            <div className="mt-8 bg-muted p-4 rounded-lg font-mono text-sm"><code>wt tree add frontend --branch-strategy feature-branch</code></div>
          </div>
        </section>

        {/* Section 4: Branches, Merging & Protection */}
        <section id="branches" className="py-20 border-b">
          <div className="container mx-auto px-4 max-w-5xl">
            <h2 className="text-3xl font-bold mb-10">Branches, Merging & Protection</h2>
            <div className="grid md:grid-cols-2 lg:grid-cols-4 gap-6">
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Per-Tree Namespaces</h3><p className="text-sm text-muted-foreground">Branches scoped to trees, not global.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Linked Branches</h3><p className="text-sm text-muted-foreground">Atomic multi-tree features that must merge together.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Branch Protection</h3><p className="text-sm text-muted-foreground">Required reviews, CI gates, signature requirements.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Merge Requests</h3><p className="text-sm text-muted-foreground">Full lifecycle — open → review → approve → merge.</p></div>
            </div>
            <div className="mt-8 bg-muted p-4 rounded-lg font-mono text-sm"><code>wt merge-request create --source feat/login --target main</code></div>
          </div>
        </section>

        {/* Section 5: Multi-Tenancy & IAM */}
        <section id="iam" className="py-20 border-b bg-muted/10">
          <div className="container mx-auto px-4 max-w-5xl">
            <Badge className="mb-4">Enterprise-Grade Access Control</Badge>
            <h2 className="text-3xl font-bold mb-4">Built-in identity, access control, and multi-tenancy.</h2>
            <p className="text-muted-foreground mb-10">Git has zero built-in identity or access control. W0rkTree enforces them at the protocol level.</p>
            <div className="grid md:grid-cols-2 lg:grid-cols-4 gap-6">
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Tenant Model</h3><p className="text-sm text-muted-foreground">Users and orgs as first-class tenants.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">5 Built-in Roles</h3><p className="text-sm text-muted-foreground">Owner → Admin → Maintainer → Developer → Viewer.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">20+ Permissions</h3><p className="text-sm text-muted-foreground">Tree, Branch, Snapshot, Sync, Management, Admin scopes.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Worktree Visibility</h3><p className="text-sm text-muted-foreground">Private (default), Shared, Public.</p></div>
            </div>
            <div className="mt-8 bg-muted p-4 rounded-lg font-mono text-sm"><code>wt access grant @frontend-team write --tree frontend</code></div>
          </div>
        </section>

        {/* Section 6: Declarative Access Control */}
        <section id="access" className="py-20 border-b">
          <div className="container mx-auto px-4 max-w-5xl">
            <Badge className="mb-4">Terraform-Style Config</Badge>
            <h2 className="text-3xl font-bold mb-10">Access control as version-controlled TOML files.</h2>
            <div className="grid md:grid-cols-3 gap-6">
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Explicit Path Registration</h3><p className="text-sm text-muted-foreground">No globs. Predictable, auditable, O(1).</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Declarative Policies</h3><p className="text-sm text-muted-foreground">Version-controlled, synced, server-enforced.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Scope Hierarchy</h3><p className="text-sm text-muted-foreground">Global → Tenant → Tree → Branch → RegisteredPath.</p></div>
            </div>
          </div>
        </section>

        {/* Section 7: License Compliance */}
        <section id="licensing" className="py-20 border-b bg-muted/10">
          <div className="container mx-auto px-4 max-w-5xl">
            <Badge className="mb-4">Protocol-Level Enforcement</Badge>
            <h2 className="text-3xl font-bold mb-4">Per-path license compliance, enforced by the server.</h2>
            <p className="text-muted-foreground mb-10">W0rkTree prevents license violations at the protocol level before they happen.</p>
            <div className="grid md:grid-cols-3 gap-6">
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">SPDX Licenses</h3><p className="text-sm text-muted-foreground">Assign licenses per path — MIT, Apache-2.0, proprietary.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">License Grants</h3><p className="text-sm text-muted-foreground">Grant access at 3 levels — read-only, modify, redistribute.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Server Enforcement</h3><p className="text-sm text-muted-foreground">Server blocks unauthorized export, fork, sync of proprietary code.</p></div>
            </div>
            <div className="mt-8 bg-muted p-4 rounded-lg font-mono text-sm"><code>wt license show</code></div>
          </div>
        </section>

        {/* Section 8: Background Process & Automation */}
        <section id="automation" className="py-20 border-b">
          <div className="container mx-auto px-4 max-w-5xl">
            <Badge className="mb-4">Automatic by Default</Badge>
            <h2 className="text-3xl font-bold mb-10">A background process that handles the tedium.</h2>
            <div className="grid md:grid-cols-4 gap-6">
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Auto-Snapshots</h3><p className="text-sm text-muted-foreground">Configurable triggers based on timeout, bytes, etc.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Filesystem Watcher</h3><p className="text-sm text-muted-foreground">Platform-native, debounced, ignore-aware.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Auto-Merge</h3><p className="text-sm text-muted-foreground">Non-conflicting remote changes merged automatically.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Crash Recovery</h3><p className="text-sm text-muted-foreground">Journal-based recovery, fsck on startup.</p></div>
            </div>
            <div className="mt-8 bg-muted p-4 rounded-lg font-mono text-sm"><code>wt worker start</code></div>
          </div>
        </section>

        {/* Section 9: Sync Protocol */}
        <section id="sync" className="py-20 border-b bg-muted/10">
          <div className="container mx-auto px-4 max-w-5xl">
            <Badge className="mb-4">Always Converging</Badge>
            <h2 className="text-3xl font-bold mb-10">Three operations. Zero confusion.</h2>
            <div className="grid md:grid-cols-3 gap-6">
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Staged Sync</h3><p className="text-sm text-muted-foreground">Automatic uploads for team visibility.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Branch Push</h3><p className="text-sm text-muted-foreground">Explicit push to canonical branch history.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Branch Pull</h3><p className="text-sm text-muted-foreground">Remote updates sync continuously.</p></div>
            </div>
          </div>
        </section>

        {/* Section 10: Configuration Model */}
        <section id="config" className="py-20 border-b">
          <div className="container mx-auto px-4 max-w-5xl">
            <Badge className="mb-4">Convention Over Configuration</Badge>
            <h2 className="text-3xl font-bold mb-10">Two folders. Complete control.</h2>
            <div className="grid md:grid-cols-2 gap-6">
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">.wt/ (Root)</h3><p className="text-sm text-muted-foreground">Project-wide config, access, identity. The ceiling.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">.wt-tree/ (Per-Tree)</h3><p className="text-sm text-muted-foreground">Tree-specific overrides. Can restrict, never expand.</p></div>
            </div>
          </div>
        </section>

        {/* Section 11: Safety & Recovery */}
        <section id="safety" className="py-20 border-b bg-muted/10">
          <div className="container mx-auto px-4 max-w-5xl">
            <Badge className="mb-4">Non-Destructive by Design</Badge>
            <h2 className="text-3xl font-bold mb-10">No rebase. No force push. No lost work.</h2>
            <div className="grid md:grid-cols-3 gap-6">
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Append-Only History</h3><p className="text-sm text-muted-foreground">Snapshots are immutable.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Reflog</h3><p className="text-sm text-muted-foreground">Full operation log — synced to server.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Revert</h3><p className="text-sm text-muted-foreground">Creates new inverse snapshots without rewriting history.</p></div>
            </div>
            <div className="mt-8 bg-muted p-4 rounded-lg font-mono text-sm"><code>wt revert &lt;snapshot-id&gt;</code></div>
          </div>
        </section>

        {/* Section 12: Large File & Storage */}
        <section id="storage" className="py-20 border-b">
          <div className="container mx-auto px-4 max-w-5xl">
            <Badge className="mb-4">No LFS Required</Badge>
            <h2 className="text-3xl font-bold mb-10">Large files are just files.</h2>
            <div className="grid md:grid-cols-3 gap-6">
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Native Chunked Storage</h3><p className="text-sm text-muted-foreground">FastCDC algorithm, content-defined boundaries.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Lazy Loading</h3><p className="text-sm text-muted-foreground">Content served on demand via virtual filesystem.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Cross-Version Dedup</h3><p className="text-sm text-muted-foreground">~70% storage savings.</p></div>
            </div>
          </div>
        </section>

        {/* Section 13: Git Compatibility */}
        <section id="git" className="py-20 border-b bg-muted/10">
          <div className="container mx-auto px-4 max-w-5xl">
            <h2 className="text-3xl font-bold mb-10">Git Compatibility</h2>
            <div className="grid md:grid-cols-3 gap-6">
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Import & Export</h3><p className="text-sm text-muted-foreground">Full migration tooling.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Remote Bridge</h3><p className="text-sm text-muted-foreground">Push/pull to Git remotes.</p></div>
              <div className="p-6 bg-card border rounded-xl"><h3 className="font-bold mb-2">Live Mirror</h3><p className="text-sm text-muted-foreground">Real-time bidirectional sync with Git remotes.</p></div>
            </div>
          </div>
        </section>

        {/* Section 14: CTA */}
        <section className="py-20">
          <div className="container mx-auto px-4 text-center max-w-4xl">
            <h2 className="text-4xl font-bold mb-8">Ready to evolve your workflow?</h2>
            <div className="flex justify-center gap-4">
              <Button size="lg" asChild><Link href="/guides/quick-start">Get Started</Link></Button>
              <Button size="lg" variant="outline" asChild><Link href="/docs/protocol">Read the Protocol</Link></Button>
            </div>
          </div>
        </section>
      </main>
    </div>
  );
}
