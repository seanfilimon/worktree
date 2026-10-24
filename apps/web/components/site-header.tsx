"use client";

import * as React from "react";
import Link from "next/link";
import Image from "next/image";
import {
  Menu,
  FolderTree,
  Network,
  GitMerge,
  GitBranch,
  GitCommit,
  Clock,
  Boxes,
  Shield,
  Rocket,
  FileText,
  Terminal,
  Code2,
  Layers,
  ArrowRightLeft,
  BookOpen,
  Newspaper,
  Users,
  Map,
  DollarSign,
  Sparkles,
  BookMarked,
  Library,
  Zap,
  Lock,
  Workflow,
  Database,
  Eye,
  ShieldCheck,
  KeyRound,
  Gauge,
  HardDrive,
  Cpu,
  RefreshCw,
  Bell,
  ListChecks,
  Kanban,
  Diff,
  PackageCheck,
  Plug,
  Server,
  Wrench,
  Calendar,
  User,
  BookText,
  type LucideIcon,
} from "lucide-react";
import { GeistMono } from "geist/font/mono";
import { ThemeToggle } from "@/components/theme-toggle";
import {
  NavigationMenu,
  NavigationMenuContent,
  NavigationMenuItem,
  NavigationMenuLink,
  NavigationMenuList,
  NavigationMenuTrigger,
  navigationMenuTriggerStyle,
} from "@/components/ui/navigation-menu";
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import { cn } from "@/lib/utils";

// ─── Shared Tab Types ────────────────────────────────────────────────────────

interface TabItem {
  title: string;
  description: string;
  href: string;
  icon: LucideIcon;
}

interface Tab {
  id: string;
  label: string;
  icon: LucideIcon;
  tagline: string;
  items: TabItem[];
}

// ─── Feature Tabs Data ───────────────────────────────────────────────────────

const featureTabs: Tab[] = [
  {
    id: "core-protocol",
    label: "Core Protocol",
    icon: Network,
    tagline: "Version control rebuilt from the protocol up",
    items: [
      { title: "Nested Trees", description: "Independent versioning, branches, access per tree", href: "/features#trees", icon: FolderTree },
      { title: "Snapshots & History", description: "BLAKE3 content-addressed, immutable, append-only", href: "/features#trees", icon: Database },
      { title: "Branches & Merging", description: "Per-tree namespaces, linked branches, merge requests", href: "/features#branches", icon: GitMerge },
      { title: "Sync Protocol", description: "Staged sync, explicit push, automatic pull, QUIC transport", href: "/features#sync", icon: RefreshCw },
    ],
  },
  {
    id: "collaboration",
    label: "Collaboration",
    icon: Users,
    tagline: "See what your team is working on",
    items: [
      { title: "Staged Visibility", description: "See what your team is working on in real-time", href: "/features#staged-visibility", icon: Eye },
      { title: "Auto Tracking", description: "Background process, auto-snapshots, auto-merge", href: "/features#automation", icon: Clock },
      { title: "Branch Protection", description: "Required reviews, CI gates, merge request system", href: "/features#branches", icon: ShieldCheck },
      { title: "Dependency Graph", description: "Cross-tree dependencies, automatic TODOs, build ordering", href: "/features#trees", icon: Code2 },
    ],
  },
  {
    id: "access-governance",
    label: "Access & Governance",
    icon: Shield,
    tagline: "Built-in identity and access control",
    items: [
      { title: "Multi-Tenancy", description: "Users, orgs, teams as first-class tenants", href: "/features#iam", icon: User },
      { title: "Declarative Access", description: "TOML-based RBAC + ABAC, version-controlled", href: "/features#access", icon: KeyRound },
      { title: "License Compliance", description: "Per-path SPDX, server-enforced, export control", href: "/features#licensing", icon: FileText },
      { title: "Audit Log", description: "Immutable, append-only, cryptographic verification", href: "/features#iam", icon: ListChecks },
    ],
  },
  {
    id: "infrastructure",
    label: "Infrastructure",
    icon: Server,
    tagline: "High-performance architecture",
    items: [
      { title: "Two-Runtime Model", description: "BGProcess (local) + Server (remote)", href: "/guides/architecture", icon: Layers },
      { title: "Configuration", description: ".wt/ root + .wt-tree/ per tree", href: "/features#config", icon: Wrench },
      { title: "Storage Engine", description: "BLAKE3, FastCDC chunking, lazy loading, dedup", href: "/features#storage", icon: Database },
      { title: "Safety & Recovery", description: "Reflog, revert, append-only — no lost work", href: "/features#safety", icon: HardDrive },
    ],
  },
  {
    id: "performance-compat",
    label: "Performance & Compat",
    icon: Zap,
    tagline: "Fast and Git-compatible",
    items: [
      { title: "Rust Engine", description: "Pure Rust, zero-copy reads, parallel operations", href: "/features#performance", icon: Cpu },
      { title: "Large Files", description: "Native chunked storage, no LFS required", href: "/features#storage", icon: PackageCheck },
      { title: "Git Import/Export", description: "Full migration tooling, live mirror mode", href: "/features#git", icon: ArrowRightLeft },
      { title: "10× Faster", description: "Sub-ms status, 10× checkout, 70% less storage", href: "/features#performance", icon: Zap },
    ],
  },
];

// ─── Documentation Tabs Data ─────────────────────────────────────────────────

const docsTabs: Tab[] = [
  {
    id: "getting-started",
    label: "Getting Started",
    icon: Rocket,
    tagline: "Start your journey",
    items: [
      { title: "Quick Start", description: "Install and run your first W0rkTree command", href: "/guides/quick-start", icon: Terminal },
      { title: "Architecture Overview", description: "Understand the core protocol concepts", href: "/guides/architecture", icon: Map },
      { title: "Configuration Model", description: "Learn how to configure .wt and .wt-tree", href: "/guides/config-model", icon: Wrench },
      { title: "Migration from Git", description: "Import your Git repositories safely", href: "/guides/migration", icon: ArrowRightLeft },
    ],
  },
  {
    id: "core-concepts",
    label: "Core Concepts",
    icon: BookOpen,
    tagline: "Deep dive into W0rkTree",
    items: [
      { title: "Staged Visibility", description: "How real-time collaboration works", href: "/guides/staged-visibility", icon: Eye },
      { title: "Multi-Tenancy", description: "Users, organizations, and team access", href: "/guides/multi-tenancy", icon: Users },
      { title: "Declarative Access", description: "Role-based and attribute-based permissions", href: "/guides/declarative-access", icon: KeyRound },
      { title: "License Compliance", description: "Per-path SPDX license enforcement", href: "/guides/license-compliance", icon: ShieldCheck },
    ],
  },
  {
    id: "references",
    label: "References",
    icon: Library,
    tagline: "Complete API and CLI documentation",
    items: [
      { title: "CLI Reference", description: "Complete list of commands and flags", href: "/docs/cli", icon: Terminal },
      { title: "Protocol Spec", description: "The official W0rkTree protocol specification", href: "/docs/protocol", icon: FileText },
      { title: "SDK Reference", description: "Rust and Node.js SDK documentation", href: "/docs/sdk", icon: Code2 },
      { title: "Server Guide", description: "Detailed server documentation", href: "/docs/server", icon: Server },
    ],
  },
  {
    id: "infrastructure",
    label: "Infrastructure",
    icon: Server,
    tagline: "Run and manage W0rkTree",
    items: [
      { title: "Server Deployment", description: "Deploy W0rkTree Server to production", href: "/docs/server/deployment", icon: Server },
      { title: "Admin Panel", description: "Manage users, tenants, and licenses", href: "/guides/admin", icon: Wrench },
      { title: "Self-Hosting", description: "Run W0rkTree on your own infrastructure", href: "/guides/self-hosting", icon: HardDrive },
      { title: "Security", description: "Security architecture and deep dive", href: "/guides/security-deep-dive", icon: Shield },
    ],
  },
];

// Flat lists for mobile menu
const allFeatures = featureTabs.flatMap((tab) => tab.items);
const allDocs = docsTabs.flatMap((tab) => tab.items);

// ─── Resources Tabs Data ─────────────────────────────────────────────────────

interface ResourcePreview {
  title: string;
  meta: string;
  href: string;
}

interface ResourceTab {
  id: string;
  label: string;
  icon: LucideIcon;
  tagline: string;
  viewAllHref: string;
  viewAllLabel: string;
  previews: ResourcePreview[];
}

const resourceTabs: ResourceTab[] = [
  {
    id: "articles",
    label: "Articles",
    icon: Newspaper,
    tagline: "Latest from the team",
    viewAllHref: "/articles",
    viewAllLabel: "All articles",
    previews: [
      {
        title: "Performance Benchmarks: W0rkTree vs Git",
        meta: "Sean F · Feb 10, 2025",
        href: "/articles/performance-benchmarks",
      },
      {
        title: "Introducing W0rkTree",
        meta: "W0rkTree Team · Jan 15, 2025",
        href: "/articles/hello-world",
      },
    ],
  },
  {
    id: "guides",
    label: "Guides",
    icon: BookText,
    tagline: "Tutorials and walkthroughs",
    viewAllHref: "/guides",
    viewAllLabel: "All guides",
    previews: [
      {
        title: "Quick Start",
        meta: "Get up and running in 5 minutes",
        href: "/guides/quick-start",
      },
      {
        title: "Migration from Git",
        meta: "Step-by-step migration guide",
        href: "/guides/migration",
      },
      {
        title: "Architecture Overview",
        meta: "High-level design and principles",
        href: "/guides/architecture",
      },
    ],
  },
  {
    id: "maintainers",
    label: "Maintainers",
    icon: Users,
    tagline: "The people behind W0rkTree",
    viewAllHref: "/maintainers",
    viewAllLabel: "All maintainers",
    previews: [
      {
        title: "Sean Filimon",
        meta: "Lead Developer & Creator of W0rkTree",
        href: "/maintainers/sean",
      },
      {
        title: "Core Team",
        meta: "Core Maintainers · Founding team",
        href: "/maintainers/core-team",
      },
    ],
  },
];

const allResources = resourceTabs.flatMap((tab) =>
  tab.previews.map((p) => ({
    title: p.title,
    description: p.meta,
    href: p.href,
    icon: tab.icon,
  })),
);

// ─── Resources Tabbed Dropdown ───────────────────────────────────────────────

function ResourcesDropdownContent() {
  const [activeTab, setActiveTab] = React.useState(resourceTabs[0].id);
  const activeTabData = resourceTabs.find((t) => t.id === activeTab)!;

  const getInitials = (name: string) =>
    name
      .split(/\s+/)
      .map((w) => w[0])
      .join("")
      .toUpperCase()
      .slice(0, 2);

  return (
    <div className="flex w-[600px] h-[280px]">
      {/* Left: Vertical Tabs */}
      <div className="flex w-[180px] shrink-0 flex-col gap-0.5 border-r border-border p-2">
        {resourceTabs.map((tab) => {
          const isActive = tab.id === activeTab;
          return (
            <button
              key={tab.id}
              onMouseEnter={() => setActiveTab(tab.id)}
              onClick={() => setActiveTab(tab.id)}
              className={cn(
                "flex items-center gap-2 rounded-md px-2.5 py-2 text-left text-[13px] transition-colors",
                isActive
                  ? "bg-accent text-accent-foreground"
                  : "text-muted-foreground hover:bg-accent/50 hover:text-foreground",
              )}
            >
              <tab.icon
                className={cn(
                  "h-3.5 w-3.5 shrink-0 transition-colors",
                  isActive ? "text-primary" : "text-muted-foreground",
                )}
              />
              <span className="font-medium">{tab.label}</span>
            </button>
          );
        })}
      </div>

      {/* Right: Preview Content */}
      <div className="flex-1 flex flex-col p-3 min-h-0">
        <div className="mb-2 flex items-center justify-between px-2 shrink-0">
          <p className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
            {activeTabData.tagline}
          </p>
        </div>
        <ul
          className={cn(
            "flex-1 overflow-y-auto min-h-0",
            activeTab === "articles" ? "grid gap-0" : "grid gap-0.5",
          )}
        >
          {activeTabData.previews.map((preview) => (
            <li key={preview.href}>
              <NavigationMenuLink asChild>
                {activeTab === "maintainers" ? (
                  <Link
                    href={preview.href}
                    className="flex items-center gap-3 select-none rounded-md px-2.5 py-2.5 leading-none no-underline outline-none transition-colors hover:bg-accent hover:text-accent-foreground focus:bg-accent focus:text-accent-foreground"
                  >
                    <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary font-bold text-xs">
                      {getInitials(preview.title)}
                    </div>
                    <div className="min-w-0">
                      <div className="text-sm font-medium leading-none mb-1">
                        {preview.title}
                      </div>
                      <p className="text-xs leading-snug text-muted-foreground line-clamp-1">
                        {preview.meta}
                      </p>
                    </div>
                  </Link>
                ) : activeTab === "articles" ? (
                  <Link
                    href={preview.href}
                    className="flex items-center gap-2 select-none rounded-md px-2 py-1.5 leading-none no-underline outline-none transition-colors hover:bg-accent hover:text-accent-foreground focus:bg-accent focus:text-accent-foreground"
                  >
                    <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md bg-primary/10">
                      <activeTabData.icon className="h-3 w-3 text-primary" />
                    </div>
                    <div className="min-w-0">
                      <div className="text-[13px] font-medium leading-none mb-0.5">
                        {preview.title}
                      </div>
                      <p className="text-[11px] leading-snug text-muted-foreground line-clamp-1">
                        {preview.meta}
                      </p>
                    </div>
                  </Link>
                ) : (
                  <Link
                    href={preview.href}
                    className="flex items-center gap-3 select-none rounded-md px-2.5 py-2.5 leading-none no-underline outline-none transition-colors hover:bg-accent hover:text-accent-foreground focus:bg-accent focus:text-accent-foreground"
                  >
                    <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-primary/10">
                      <activeTabData.icon className="h-3.5 w-3.5 text-primary" />
                    </div>
                    <div className="min-w-0">
                      <div className="text-sm font-medium leading-none mb-1">
                        {preview.title}
                      </div>
                      <p className="text-xs leading-snug text-muted-foreground line-clamp-1">
                        {preview.meta}
                      </p>
                    </div>
                  </Link>
                )}
              </NavigationMenuLink>
            </li>
          ))}
        </ul>
        <div className="border-t border-border pt-2 mt-auto px-2 shrink-0">
          <Link
            href={activeTabData.viewAllHref}
            className="flex items-center gap-2 rounded-md px-0.5 py-1.5 text-xs text-muted-foreground transition-colors hover:text-foreground"
          >
            <activeTabData.icon className="h-3 w-3" />
            {activeTabData.viewAllLabel} →
          </Link>
        </div>
      </div>
    </div>
  );
}

// ─── Reusable Tabbed Dropdown ────────────────────────────────────────────────

function TabbedDropdownContent({
  tabs,
  ctaLabel,
  ctaHref,
  ctaIcon: CtaIcon,
}: {
  tabs: Tab[];
  ctaLabel: string;
  ctaHref: string;
  ctaIcon: LucideIcon;
}) {
  const [activeTab, setActiveTab] = React.useState(tabs[0].id);
  const activeTabData = tabs.find((t) => t.id === activeTab)!;

  return (
    <div className="flex w-[600px] h-[280px]">
      {/* Left: Vertical Tabs */}
      <div className="flex w-[180px] shrink-0 flex-col gap-0.5 border-r border-border p-2">
        {tabs.map((tab) => {
          const isActive = tab.id === activeTab;
          return (
            <button
              key={tab.id}
              onMouseEnter={() => setActiveTab(tab.id)}
              onClick={() => setActiveTab(tab.id)}
              className={cn(
                "flex items-center gap-2 rounded-md px-2.5 py-2 text-left text-[13px] transition-colors",
                isActive
                  ? "bg-accent text-accent-foreground"
                  : "text-muted-foreground hover:bg-accent/50 hover:text-foreground",
              )}
            >
              <tab.icon
                className={cn(
                  "h-3.5 w-3.5 shrink-0 transition-colors",
                  isActive ? "text-primary" : "text-muted-foreground",
                )}
              />
              <span className="font-medium">{tab.label}</span>
            </button>
          );
        })}

        <div className="mt-auto border-t border-border pt-2 mt-2">
          <Link
            href={ctaHref}
            className="flex items-center gap-2 rounded-md px-2.5 py-1.5 text-xs text-muted-foreground transition-colors hover:text-foreground"
          >
            <CtaIcon className="h-3 w-3" />
            {ctaLabel}
          </Link>
        </div>
      </div>

      {/* Right: Tab Content */}
      <div className="flex-1 flex flex-col p-3">
        <div className="mb-2 px-2">
          <p className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
            {activeTabData.tagline}
          </p>
        </div>
        <ul className="grid gap-0.5">
          {activeTabData.items.map((item) => (
            <li key={item.title}>
              <NavigationMenuLink asChild>
                <Link
                  href={item.href}
                  className="flex items-center gap-3 select-none rounded-md px-2.5 py-2.5 leading-none no-underline outline-none transition-colors hover:bg-accent hover:text-accent-foreground focus:bg-accent focus:text-accent-foreground"
                >
                  <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-primary/10">
                    <item.icon className="h-3.5 w-3.5 text-primary" />
                  </div>
                  <div className="min-w-0">
                    <div className="text-sm font-medium leading-none mb-1">
                      {item.title}
                    </div>
                    <p className="text-xs leading-snug text-muted-foreground line-clamp-1">
                      {item.description}
                    </p>
                  </div>
                </Link>
              </NavigationMenuLink>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

// ─── Site Header ─────────────────────────────────────────────────────────────

export function SiteHeader() {
  const [isOpen, setIsOpen] = React.useState(false);

  return (
    <header
      className={cn(
        "sticky top-0 z-50 w-full border-b bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/60",
        GeistMono.className,
      )}
    >
      <div className="container flex h-16 max-w-screen-2xl items-center justify-between px-4">
        {/* Logo */}
        <div className="flex shrink-0">
          <Link
            href="/"
            className="flex items-center space-x-3 transition-opacity hover:opacity-80"
          >
            <Image
              src="/worktree-logo.svg"
              alt="W0rkTree Logo"
              width={50}
              height={50}
              className="h-[50px] w-[50px] dark:hidden"
            />
            <Image
              src="/worktree-logo-dark.svg"
              alt="W0rkTree Logo"
              width={50}
              height={50}
              className="h-[50px] w-[50px] hidden dark:block"
            />
            <span className="text-xl font-bold tracking-tight">W0rkTree</span>
          </Link>
        </div>

        {/* Desktop Navigation — centered */}
        <NavigationMenu className="hidden lg:flex absolute left-1/2 -translate-x-1/2">
          <NavigationMenuList>
            {/* Features Dropdown — Tabbed */}
            <NavigationMenuItem>
              <NavigationMenuTrigger>
                <Sparkles className="h-3.5 w-3.5 mr-1.5" />
                Features
              </NavigationMenuTrigger>
              <NavigationMenuContent>
                <TabbedDropdownContent
                  tabs={featureTabs}
                  ctaLabel="All features"
                  ctaHref="/features"
                  ctaIcon={Sparkles}
                />
              </NavigationMenuContent>
            </NavigationMenuItem>

            {/* Documentation Dropdown — Tabbed */}
            <NavigationMenuItem>
              <NavigationMenuTrigger>
                <BookMarked className="h-3.5 w-3.5 mr-1.5" />
                Documentation
              </NavigationMenuTrigger>
              <NavigationMenuContent>
                <TabbedDropdownContent
                  tabs={docsTabs}
                  ctaLabel="All docs"
                  ctaHref="/docs"
                  ctaIcon={BookMarked}
                />
              </NavigationMenuContent>
            </NavigationMenuItem>

            {/* Resources Dropdown — Tabbed */}
            <NavigationMenuItem>
              <NavigationMenuTrigger>
                <Library className="h-3.5 w-3.5 mr-1.5" />
                Resources
              </NavigationMenuTrigger>
              <NavigationMenuContent>
                <ResourcesDropdownContent />
              </NavigationMenuContent>
            </NavigationMenuItem>
          </NavigationMenuList>
        </NavigationMenu>

        {/* Right Side Actions */}
        <div className="flex shrink-0 items-center gap-3">
          <ThemeToggle />

          <Link
            href="/guides/quick-start"
            className="hidden md:inline-flex h-9 items-center justify-center rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground shadow-sm transition-all hover:bg-primary/90"
          >
            Get Started
          </Link>

          {/* Mobile Menu */}
          <Sheet open={isOpen} onOpenChange={setIsOpen}>
            <SheetTrigger asChild className="lg:hidden">
              <button
                className="inline-flex h-9 w-9 items-center justify-center rounded-lg border bg-background transition-colors hover:bg-accent hover:text-accent-foreground"
                aria-label="Toggle menu"
              >
                <Menu className="h-5 w-5" />
              </button>
            </SheetTrigger>
            <SheetContent side="right" className="w-[300px] sm:w-[400px]">
              <SheetHeader>
                <SheetTitle className="text-left">Navigation</SheetTitle>
              </SheetHeader>
              <nav className="mt-8 flex flex-col space-y-6">
                {/* Features Section */}
                <div>
                  <h3 className="mb-3 text-sm font-semibold">Features</h3>
                  <ul className="space-y-1">
                    {allFeatures.map((feature) => (
                      <li key={feature.title}>
                        <Link
                          href={feature.href}
                          onClick={() => setIsOpen(false)}
                          className="flex items-start gap-3 rounded-lg p-3 text-sm transition-colors hover:bg-accent"
                        >
                          <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-primary/10">
                            <feature.icon className="h-4 w-4 text-primary" />
                          </div>
                          <div className="min-w-0">
                            <div className="font-medium">{feature.title}</div>
                            <div className="text-xs text-muted-foreground">
                              {feature.description}
                            </div>
                          </div>
                        </Link>
                      </li>
                    ))}
                  </ul>
                </div>

                {/* Documentation Section */}
                <div>
                  <h3 className="mb-3 text-sm font-semibold">Documentation</h3>
                  <ul className="space-y-1">
                    {allDocs.map((doc) => (
                      <li key={doc.title}>
                        <Link
                          href={doc.href}
                          onClick={() => setIsOpen(false)}
                          className="flex items-start gap-3 rounded-lg p-3 text-sm transition-colors hover:bg-accent"
                        >
                          <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-primary/10">
                            <doc.icon className="h-4 w-4 text-primary" />
                          </div>
                          <div className="min-w-0">
                            <div className="font-medium">{doc.title}</div>
                            <div className="text-xs text-muted-foreground">
                              {doc.description}
                            </div>
                          </div>
                        </Link>
                      </li>
                    ))}
                  </ul>
                </div>

                {/* Resources Section */}
                <div>
                  <h3 className="mb-3 text-sm font-semibold">Resources</h3>
                  <ul className="space-y-1">
                    {allResources.map((resource) => (
                      <li key={resource.href}>
                        <Link
                          href={resource.href}
                          onClick={() => setIsOpen(false)}
                          className="flex items-start gap-3 rounded-lg p-3 text-sm transition-colors hover:bg-accent"
                        >
                          <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-primary/10">
                            <resource.icon className="h-4 w-4 text-primary" />
                          </div>
                          <div className="min-w-0">
                            <div className="font-medium">{resource.title}</div>
                            <div className="text-xs text-muted-foreground">
                              {resource.description}
                            </div>
                          </div>
                        </Link>
                      </li>
                    ))}
                  </ul>
                </div>

                {/* CTA Button */}
                <div className="pt-4">
                  <Link
                    href="/guides/quick-start"
                    onClick={() => setIsOpen(false)}
                    className="flex h-10 w-full items-center justify-center rounded-lg bg-primary px-4 text-sm font-semibold text-primary-foreground transition-all hover:bg-primary/90"
                  >
                    Get Started
                  </Link>
                </div>
              </nav>
            </SheetContent>
          </Sheet>
        </div>
      </div>
    </header>
  );
}

// ─── Shared ListItem for Resources ───────────────────────────────────────────

const ListItem = React.forwardRef<
  React.ElementRef<"a">,
  React.ComponentPropsWithoutRef<"a"> & {
    title: string;
    icon: React.ComponentType<{ className?: string }>;
  }
>(({ className, title, children, href, icon: Icon, ...props }, ref) => {
  return (
    <li>
      <NavigationMenuLink asChild>
        <Link
          ref={ref}
          href={href || "#"}
          className={cn(
            "flex items-start gap-3 select-none rounded-lg p-3 leading-none no-underline outline-none transition-colors hover:bg-accent hover:text-accent-foreground focus:bg-accent focus:text-accent-foreground",
            className,
          )}
          {...props}
        >
          <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-primary/10 mt-0.5">
            <Icon className="h-4 w-4 text-primary" />
          </div>
          <div className="min-w-0">
            <div className="text-sm font-medium leading-none mb-1.5">
              {title}
            </div>
            <p className="line-clamp-2 text-xs leading-snug text-muted-foreground">
              {children}
            </p>
          </div>
        </Link>
      </NavigationMenuLink>
    </li>
  );
});
ListItem.displayName = "ListItem";
