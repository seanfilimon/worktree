"use client";

import * as React from "react";
import Link from "next/link";
import {
  FolderTree,
  Database,
  Eye,
  Clock,
  RefreshCw,
  User,
  FileText,
  ShieldCheck,
  HardDrive,
  ArrowRightLeft,
} from "lucide-react";
import { motion, AnimatePresence } from "motion/react";
import {
  IoEye,
  IoPeople,
  IoStatsChart,
  IoArrowForward,
  IoShieldCheckmark,
  IoTime,
  IoGitNetwork,
  IoGitBranch,
  IoGitMerge,
  IoCube,
  IoFlash,
  IoCheckmarkCircle,
  IoStar,
  IoLogoGithub,
  IoLogoTwitter,
  IoLogoDiscord,
  IoTerminal,
  IoCopy,
  IoCheckmark,
  IoSpeedometer,
  IoServer,
  IoHardwareChip,
  IoHelpCircle,
  IoSwapHorizontal,
  IoExtensionPuzzle,
  IoSchool,
  IoChatbubbleEllipses,
  IoMail,
  IoDocumentText,
  IoBook,
  IoHeart,
} from "react-icons/io5";
import { SiteHeader } from "@/components/site-header";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Separator } from "@/components/ui/separator";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import {
  HoverCard,
  HoverCardContent,
  HoverCardTrigger,
} from "@/components/ui/hover-card";
import { Progress } from "@/components/ui/progress";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { TooltipProvider } from "@/components/ui/tooltip";

export default function HomePage() {
  const [copiedCommand, setCopiedCommand] = React.useState<string | null>(null);
  const [activeTab, setActiveTab] = React.useState<string>("wt");

  const copyToClipboard = (text: string, id: string) => {
    navigator.clipboard.writeText(text);
    setCopiedCommand(id);
    setTimeout(() => setCopiedCommand(null), 2000);
  };

  return (
    <div className="flex min-h-screen flex-col bg-background">
      <SiteHeader />

      <main className="flex-1">
        {/* Hero Section */}
        <section className="relative overflow-hidden border-b">
          <div className="absolute inset-0 bg-gradient-to-b from-muted/50 to-background" />
          <div className="absolute inset-0 bg-grid-white/[0.02] bg-[size:60px_60px]" />

          <div className="relative container mx-auto px-4 py-24 md:py-32">
            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.5, ease: "easeOut" }}
              className="mx-auto max-w-4xl text-center"
            >
              <motion.div
                initial={{ opacity: 0, scale: 0.9 }}
                animate={{ opacity: 1, scale: 1 }}
                transition={{ duration: 0.3, delay: 0.1 }}
              >
                <Badge variant="outline" className="mb-6">
                  <span className="relative flex h-2 w-2 mr-2">
                    <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-primary opacity-75" />
                    <span className="relative inline-flex h-2 w-2 rounded-full bg-primary" />
                  </span>
                  The Post-Git Protocol
                </Badge>
              </motion.div>

              <motion.h1
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.5, delay: 0.2 }}
                className="mb-6 text-4xl font-bold tracking-tight sm:text-6xl md:text-7xl"
              >
                Multi-Tenant
                <br />
                <span className="text-primary">Version Control.</span>
              </motion.h1>

              <motion.p
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.5, delay: 0.3 }}
                className="mx-auto mb-8 max-w-2xl text-lg text-muted-foreground md:text-xl"
              >
                A modern version control system designed for teams. Simple,
                fast, and Git-compatible.
              </motion.p>

              <motion.div
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.5, delay: 0.4 }}
                className="flex flex-col items-center justify-center gap-4 sm:flex-row mb-12"
              >
                <Button size="lg" className="gap-2 group" asChild>
                  <Link href="/guides/quick-start">
                    Get Started
                    <IoArrowForward className="h-4 w-4 transition-transform group-hover:translate-x-1" />
                  </Link>
                </Button>
                <Button size="lg" variant="outline" className="gap-2" asChild>
                  <Link href="/docs/protocol">
                    <IoGitBranch className="h-4 w-4" />
                    View Protocol
                  </Link>
                </Button>
              </motion.div>

              {/* Installation Commands */}
              <motion.div
                initial={{ opacity: 0, y: 30 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.6, delay: 0.5 }}
                className="mx-auto max-w-2xl"
              >
                <Card>
                  <CardHeader className="pb-3">
                    <CardTitle className="text-lg flex items-center gap-2">
                      <IoTerminal className="h-5 w-5" />
                      Quick Install
                    </CardTitle>
                  </CardHeader>
                  <CardContent>
                    <Tabs defaultValue="npm" className="w-full">
                      <TabsList className="grid w-full grid-cols-4">
                        <TabsTrigger value="npm">npm</TabsTrigger>
                        <TabsTrigger value="pnpm">pnpm</TabsTrigger>
                        <TabsTrigger value="cargo">Cargo</TabsTrigger>
                        <TabsTrigger value="binary">Binary</TabsTrigger>
                      </TabsList>
                      <TabsContent value="npm" className="mt-4">
                        <div className="relative rounded-lg bg-muted p-4 font-mono text-sm">
                          <code>npm install -g @worktree/cli</code>
                          <Button
                            size="sm"
                            variant="ghost"
                            className="absolute right-2 top-2"
                            onClick={() =>
                              copyToClipboard(
                                "npm install -g @worktree/cli",
                                "hero-npm",
                              )
                            }
                          >
                            {copiedCommand === "hero-npm" ? (
                              <IoCheckmark className="h-4 w-4" />
                            ) : (
                              <IoCopy className="h-4 w-4" />
                            )}
                          </Button>
                        </div>
                      </TabsContent>
                      <TabsContent value="pnpm" className="mt-4">
                        <div className="relative rounded-lg bg-muted p-4 font-mono text-sm">
                          <code>pnpm add -g @worktree/cli</code>
                          <Button
                            size="sm"
                            variant="ghost"
                            className="absolute right-2 top-2"
                            onClick={() =>
                              copyToClipboard(
                                "pnpm add -g @worktree/cli",
                                "hero-pnpm",
                              )
                            }
                          >
                            {copiedCommand === "hero-pnpm" ? (
                              <IoCheckmark className="h-4 w-4" />
                            ) : (
                              <IoCopy className="h-4 w-4" />
                            )}
                          </Button>
                        </div>
                      </TabsContent>
                      <TabsContent value="cargo" className="mt-4">
                        <div className="relative rounded-lg bg-muted p-4 font-mono text-sm">
                          <code>cargo install worktree</code>
                          <Button
                            size="sm"
                            variant="ghost"
                            className="absolute right-2 top-2"
                            onClick={() =>
                              copyToClipboard(
                                "cargo install worktree",
                                "hero-cargo",
                              )
                            }
                          >
                            {copiedCommand === "hero-cargo" ? (
                              <IoCheckmark className="h-4 w-4" />
                            ) : (
                              <IoCopy className="h-4 w-4" />
                            )}
                          </Button>
                        </div>
                      </TabsContent>
                      <TabsContent value="binary" className="mt-4">
                        <div className="relative rounded-lg bg-muted p-4 font-mono text-sm">
                          <code>curl -fsSL https://get.worktree.dev | sh</code>
                          <Button
                            size="sm"
                            variant="ghost"
                            className="absolute right-2 top-2"
                            onClick={() =>
                              copyToClipboard(
                                "curl -fsSL https://get.worktree.dev | sh",
                                "hero-binary",
                              )
                            }
                          >
                            {copiedCommand === "hero-binary" ? (
                              <IoCheckmark className="h-4 w-4" />
                            ) : (
                              <IoCopy className="h-4 w-4" />
                            )}
                          </Button>
                        </div>
                      </TabsContent>
                    </Tabs>
                  </CardContent>
                </Card>
              </motion.div>
            </motion.div>
          </div>
        </section>

        {/* Features Section — Connected Protocol Architecture */}
        <section className="border-b py-16 md:py-24">
          <div className="w-full">
            <div className="text-center mb-16 max-w-3xl mx-auto px-4">
              <Badge variant="secondary" className="mb-4">
                Features
              </Badge>
              <h2 className="text-3xl font-bold mb-4">Protocol Architecture</h2>
              <p className="text-lg text-muted-foreground">
                Multi-tenant version control with built-in coordination
              </p>
            </div>

            {/* 10-Card Grid */}
            <motion.div
              initial={{ opacity: 0, y: 30 }}
              whileInView={{ opacity: 1, y: 0 }}
              viewport={{ once: true }}
              transition={{ duration: 0.5 }}
              className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-5 w-full"
            >
              {[
                {
                  title: "Nested Trees",
                  badge: "Core",
                  cmd: "wt tree add frontend",
                  desc: "Independent versioning, branches, and access rules per tree.",
                  Icon: FolderTree,
                },
                {
                  title: "Snapshots & History",
                  badge: "Core",
                  cmd: 'wt snapshot -m "feat: add login"',
                  desc: "BLAKE3 content-addressed, immutable, and append-only DAG.",
                  Icon: Database,
                },
                {
                  title: "Staged Visibility",
                  badge: "Collaboration",
                  cmd: "wt status --team",
                  desc: "See what your team is working on before they push.",
                  Icon: Eye,
                },
                {
                  title: "Auto Tracking",
                  badge: "Automation",
                  cmd: "wt worker start",
                  desc: "Background worker handles snapshots and file watching.",
                  Icon: Clock,
                },
                {
                  title: "Sync Protocol",
                  badge: "Infrastructure",
                  cmd: "wt push",
                  desc: "QUIC transport with staged sync and explicit push.",
                  Icon: RefreshCw,
                },
                {
                  title: "Multi-Tenant IAM",
                  badge: "Security",
                  cmd: "wt access grant @team read",
                  desc: "Built-in roles, ABAC/RBAC, and cross-tenant grants.",
                  Icon: User,
                },
                {
                  title: "License Compliance",
                  badge: "Governance",
                  cmd: "wt license show",
                  desc: "Per-path SPDX compliance, enforced by the server.",
                  Icon: FileText,
                },
                {
                  title: "Branch Protection",
                  badge: "Governance",
                  cmd: "wt merge-request create",
                  desc: "Required reviews, CI gates, and linked branch validation.",
                  Icon: ShieldCheck,
                },
                {
                  title: "Large File Handling",
                  badge: "Performance",
                  cmd: "Native — no LFS needed",
                  desc: "FastCDC chunking, dedup, and lazy loading built-in.",
                  Icon: HardDrive,
                },
                {
                  title: "Git Compatibility",
                  badge: "Migration",
                  cmd: "wt init --from-git ./repo",
                  desc: "Import, export, and live mirror with Git remotes.",
                  Icon: ArrowRightLeft,
                },
              ].map((card, i) => (
                <div
                  key={card.title}
                  className="bg-card p-5 border border-border flex flex-col items-start gap-4 -ml-px -mt-px"
                >
                  <div className="flex w-full justify-between items-start">
                    <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-primary/10 text-primary">
                      <card.Icon className="h-5 w-5" />
                    </div>
                    <Badge
                      variant="outline"
                      className="text-[10px] uppercase tracking-wider"
                    >
                      {card.badge}
                    </Badge>
                  </div>
                  <div>
                    <h3 className="text-base font-semibold mb-1">
                      {card.title}
                    </h3>
                    <p className="text-sm text-muted-foreground">{card.desc}</p>
                  </div>
                  <code className="text-[10px] bg-muted px-2 py-1.5 rounded font-mono text-muted-foreground mt-auto w-full truncate">
                    {card.cmd}
                  </code>
                </div>
              ))}
            </motion.div>
          </div>
        </section>

        {/* Why Choose W0rkTree */}
        <section className="border-b py-16 md:py-24 bg-muted/30">
          <div className="w-full">
            <div className="text-center mb-12 max-w-3xl mx-auto px-4">
              <Badge variant="secondary" className="mb-4">
                Why W0rkTree
              </Badge>
              <h2 className="text-3xl font-bold mb-4">
                Stop fighting your tools
              </h2>
              <p className="text-lg text-muted-foreground max-w-xl mx-auto">
                A custom protocol built for multi-team collaboration.
              </p>
            </div>

            <div className="w-full">
              <div className="flex justify-center mb-8 px-4">
                <div className="relative flex w-full max-w-[400px] rounded-lg bg-muted/50 p-1">
                  <button
                    onClick={() => setActiveTab("git")}
                    className={`relative z-10 flex w-full items-center justify-center py-2 text-[11px] font-semibold uppercase tracking-wider transition-colors ${
                      activeTab === "git"
                        ? "text-destructive"
                        : "text-muted-foreground hover:text-foreground"
                    }`}
                  >
                    <IoCopy className="mr-1.5 h-3.5 w-3.5" />
                    Git Problem
                  </button>
                  <button
                    onClick={() => setActiveTab("wt")}
                    className={`relative z-10 flex w-full items-center justify-center py-2 text-[11px] font-semibold uppercase tracking-wider transition-colors ${
                      activeTab === "wt"
                        ? "text-primary"
                        : "text-muted-foreground hover:text-foreground"
                    }`}
                  >
                    <IoFlash className="mr-1.5 h-3.5 w-3.5" />
                    W0rkTree Answer
                  </button>
                  <motion.div
                    className={`absolute inset-y-1 w-[calc(50%-4px)] rounded-md shadow-sm ${
                      activeTab === "git"
                        ? "bg-destructive/10"
                        : "bg-primary/10"
                    }`}
                    initial={false}
                    animate={{
                      left: activeTab === "git" ? "4px" : "calc(50%)",
                    }}
                    transition={{ type: "spring", bounce: 0.2, duration: 0.5 }}
                  />
                </div>
              </div>

              <motion.div
                initial={{ opacity: 0, y: 20 }}
                whileInView={{ opacity: 1, y: 0 }}
                viewport={{ once: true }}
                transition={{ duration: 0.4 }}
                className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-4 w-full"
              >
                {[
                  {
                    cat: "UX & Workflow",
                    icon: IoTerminal,
                    color: "text-blue-500",
                    bg: "bg-blue-500/10",
                    git: "Git has 150+ commands, many overloaded (e.g., checkout). The staging area is an unnecessary intermediary.",
                    wt: "W0rkTree provides one clear command per operation. Changes are tracked automatically without a staging area.",
                  },
                  {
                    cat: "Safety & Recovery",
                    icon: IoShieldCheckmark,
                    color: "text-emerald-500",
                    bg: "bg-emerald-500/10",
                    git: "Commands like `git reset --hard` and `git push --force` destroy work and rewrite history permanently.",
                    wt: "History is append-only and immutable. Reverts create new snapshots, and deleted branches have recovery windows.",
                  },
                  {
                    cat: "Collaboration",
                    icon: IoPeople,
                    color: "text-purple-500",
                    bg: "bg-purple-500/10",
                    git: "Work remains entirely invisible to the team until explicitly pushed, leading to surprise merge conflicts.",
                    wt: "Staged snapshots sync automatically in the background, providing real-time team visibility into in-flight work.",
                  },
                  {
                    cat: "Identity & Access",
                    icon: User,
                    color: "text-amber-500",
                    bg: "bg-amber-500/10",
                    git: "Native Git has zero authentication, authorization, or access control. It relies entirely on hosting platforms.",
                    wt: "Protocol-level IAM with tenants, RBAC/ABAC policies, and granular path-based access control built-in.",
                  },
                  {
                    cat: "Security & Transport",
                    icon: IoServer,
                    color: "text-rose-500",
                    bg: "bg-rose-500/10",
                    git: "The native git:// protocol is unencrypted and unauthenticated, while SSH/HTTPS are bolted-on afterthoughts.",
                    wt: "Built on QUIC with mandatory TLS 1.3, multiplexed streams, 0-RTT reconnects, and built-in secret scanning.",
                  },
                  {
                    cat: "Large Files & Scale",
                    icon: HardDrive,
                    color: "text-cyan-500",
                    bg: "bg-cyan-500/10",
                    git: "Large binaries bloat the repository forever, requiring external workarounds like Git LFS.",
                    wt: "Native content-defined chunking (FastCDC), cross-version deduplication, and lazy loading via virtual filesystems.",
                  },
                  {
                    cat: "License Compliance",
                    icon: IoDocumentText,
                    color: "text-yellow-500",
                    bg: "bg-yellow-500/10",
                    git: "Open-source and proprietary code mix freely. Compliance is entirely a legal honor system.",
                    wt: "Per-path SPDX license tracking. The protocol actively blocks unauthorized export, sync, and fork operations.",
                  },
                  {
                    cat: "Multi-Project Org",
                    icon: FolderTree,
                    color: "text-indigo-500",
                    bg: "bg-indigo-500/10",
                    git: "Submodules are notoriously brittle and detached. Monorepos require complex sparse-checkout rules.",
                    wt: "Nested trees act as independent, versioned namespaces with their own branches, access rules, and licenses.",
                  },
                ].map((row, i) => (
                  <div
                    key={row.cat}
                    className="bg-card p-6 border border-border flex flex-col gap-4 -ml-px -mt-px"
                  >
                    <div className="flex items-center gap-3">
                      <div
                        className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-lg ${row.bg} ${row.color}`}
                      >
                        <row.icon className="h-5 w-5" />
                      </div>
                      <h3 className="text-base font-bold text-foreground">
                        {row.cat}
                      </h3>
                    </div>
                    <div className="mt-auto min-h-[110px]">
                      <AnimatePresence mode="wait">
                        {activeTab === "git" ? (
                          <motion.div
                            key="git"
                            initial={{ opacity: 0, filter: "blur(4px)" }}
                            animate={{ opacity: 1, filter: "blur(0px)" }}
                            exit={{ opacity: 0, filter: "blur(4px)" }}
                            transition={{ duration: 0.2 }}
                            className="text-sm text-muted-foreground leading-relaxed p-4 rounded-lg bg-destructive/5 border border-destructive/10 h-full"
                          >
                            {row.git}
                          </motion.div>
                        ) : (
                          <motion.div
                            key="wt"
                            initial={{ opacity: 0, filter: "blur(4px)" }}
                            animate={{ opacity: 1, filter: "blur(0px)" }}
                            exit={{ opacity: 0, filter: "blur(4px)" }}
                            transition={{ duration: 0.2 }}
                            className="text-sm text-foreground leading-relaxed p-4 rounded-lg bg-primary/5 border border-primary/10 h-full"
                          >
                            {row.wt}
                          </motion.div>
                        )}
                      </AnimatePresence>
                    </div>
                  </div>
                ))}
              </motion.div>
            </div>
          </div>
        </section>

        {/* Performance Section */}
        <section className="border-b py-16 md:py-24 bg-muted/10">
          <div className="w-full">
            <div className="text-center mb-12 max-w-3xl mx-auto px-4">
              <Badge variant="secondary" className="mb-4">
                Performance
              </Badge>
              <h2 className="text-3xl font-bold mb-4">
                Built for Speed & Scale
              </h2>
              <p className="text-lg text-muted-foreground flex items-center justify-center gap-2">
                <IoHardwareChip className="h-4 w-4" />
                Apple M2 · 16GB · 500k files (*Projected Benchmarks)
              </p>
            </div>

            <motion.div
              initial={{ opacity: 0, y: 20 }}
              whileInView={{ opacity: 1, y: 0 }}
              viewport={{ once: true }}
              transition={{ duration: 0.4 }}
              className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-5 w-full"
            >
              {[
                {
                  label: "Checkout",
                  description: "Branch & tree switching",
                  icon: IoSpeedometer,
                  wt: "0.2s",
                  git: "2.1s",
                  multiplier: "10×",
                },
                {
                  label: "Status",
                  description: "Working tree query",
                  icon: IoTime,
                  wt: "<1ms",
                  git: "~50ms",
                  multiplier: "50×",
                },
                {
                  label: "Dep. Resolution",
                  description: "Cross-tree graph traversal",
                  icon: IoGitBranch,
                  wt: "3ms",
                  git: "—",
                  multiplier: "∞",
                },
                {
                  label: "Large File Sync",
                  description: "Chunk dedup",
                  icon: IoGitNetwork,
                  wt: "Chunked",
                  git: "Full",
                  multiplier: "~80% savings",
                },
                {
                  label: "Storage",
                  description: "Dedup efficiency",
                  icon: IoServer,
                  wt: "1×",
                  git: "3.2×",
                  multiplier: "70%",
                },
              ].map((bench, i) => (
                <div
                  key={bench.label}
                  className="bg-card p-6 border border-border flex flex-col items-start gap-4 -ml-px -mt-px"
                >
                  <div className="flex w-full justify-between items-start">
                    <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-primary/10 text-primary">
                      <bench.icon className="h-5 w-5" />
                    </div>
                    <Badge
                      variant="outline"
                      className="text-[10px] uppercase tracking-wider text-emerald-500 border-emerald-500/30 bg-emerald-500/10"
                    >
                      {bench.multiplier}
                    </Badge>
                  </div>
                  <div>
                    <h3 className="text-base font-semibold mb-1">
                      {bench.label}
                    </h3>
                    <p className="text-sm text-muted-foreground">
                      {bench.description}
                    </p>
                  </div>
                  <div className="mt-auto pt-4 w-full grid grid-cols-2 gap-2 text-sm border-t border-border">
                    <div className="flex flex-col">
                      <span className="text-[10px] text-muted-foreground uppercase tracking-wider mb-1">
                        W0rkTree
                      </span>
                      <span className="font-mono text-primary font-medium">
                        {bench.wt}
                      </span>
                    </div>
                    <div className="flex flex-col">
                      <span className="text-[10px] text-muted-foreground uppercase tracking-wider mb-1">
                        Git
                      </span>
                      <span className="font-mono text-muted-foreground line-through decoration-destructive/50">
                        {bench.git}
                      </span>
                    </div>
                  </div>
                </div>
              ))}
            </motion.div>
          </div>
        </section>

        {/* FAQ Section — two-column with icons */}
        <section className="border-b py-16 md:py-24 bg-muted/30">
          <div className="container mx-auto px-4">
            <div className="mx-auto max-w-5xl">
              <div className="text-center mb-14">
                <Badge variant="secondary" className="mb-4">
                  <IoHelpCircle className="h-3 w-3 mr-1" />
                  FAQ
                </Badge>
                <h2 className="text-3xl font-bold mb-4">Common Questions</h2>
                <p className="text-lg text-muted-foreground max-w-2xl mx-auto">
                  Everything you need to know before getting started
                </p>
              </div>

              <motion.div
                initial={{ opacity: 0, y: 20 }}
                whileInView={{ opacity: 1, y: 0 }}
                viewport={{ once: true }}
                transition={{ duration: 0.4 }}
                className="rounded-xl border border-border overflow-hidden"
              >
                {[
                  {
                    icon: IoGitNetwork,
                    q: "Is W0rkTree built on Git?",
                    a: "No. W0rkTree is a completely custom version control protocol built from scratch in Rust. It's not a wrapper around Git, not a Git extension, and doesn't use Git under the hood. The protocol was designed ground-up for multi-tenant tree isolation, native dependency graphs, and built-in coordination — things Git was never architected to support.",
                  },
                  {
                    icon: IoShieldCheckmark,
                    q: "How does Tree isolation work?",
                    a: "Each Tree is an independent unit in the W0rkTree protocol with its own snapshot DAG, branch namespace, and permission scope. Trees are first-class citizens — not directories in a monorepo. They link through the protocol's native dependency graph, so teams own their Tree entirely while the protocol handles cross-tree coordination automatically.",
                  },
                  {
                    icon: IoSwapHorizontal,
                    q: "Can I migrate from Git?",
                    a: "Yes. W0rkTree includes migration tooling that imports history from Git repositories and converts them into Trees. Your snapshot history is preserved and restructured into the W0rkTree protocol format. It's a one-way migration — once you're on W0rkTree, you're on a better protocol.",
                  },
                  {
                    icon: IoExtensionPuzzle,
                    q: "How does CI/CD work without Git?",
                    a: "The W0rkTree CLI integrates directly into any pipeline. The protocol's dependency graph provides build ordering natively — no scripts or hacks needed. We provide first-party plugins for major CI/CD platforms, and the protocol exposes hooks for custom automation.",
                  },
                  {
                    icon: IoSchool,
                    q: "Why not just use Git?",
                    a: "Git is a single-repo tool from 2005. It has no concept of team boundaries, dependency tracking, or cross-repo coordination. W0rkTree was purpose-built for how modern teams actually ship — isolated ownership, explicit dependencies, atomic multi-tree operations, and built-in project management. It's not Git with extras. It's the protocol that should have existed all along.",
                  },
                  {
                    icon: IoEye,
                    q: "What are staged snapshots?",
                    a: "Your in-progress work is visible to the team before you push — like a live activity feed for code. Staged ≠ pushed.",
                  },
                  {
                    icon: IoPeople,
                    q: "How does multi-tenancy work?",
                    a: "Every user/org is a tenant with username + email. Tenants own worktrees and grant access via declarative TOML config. Private by default.",
                  },
                  {
                    icon: IoDocumentText,
                    q: "Does W0rkTree handle licensing?",
                    a: "Yes — per-path SPDX licenses, enforced at the protocol level. The server blocks unauthorized export/fork/sync of proprietary code.",
                  },
                ].map((faq, i) => (
                  <details
                    key={faq.q}
                    className={`group bg-card ${
                      i > 0 ? "border-t border-border" : ""
                    }`}
                  >
                    <summary className="flex items-center gap-4 p-5 md:p-6 cursor-pointer select-none list-none [&::-webkit-details-marker]:hidden hover:bg-accent/30 transition-colors">
                      <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 ring-1 ring-primary/20">
                        <faq.icon className="h-4 w-4 text-primary" />
                      </div>
                      <span className="flex-1 text-sm font-semibold">
                        {faq.q}
                      </span>
                      <IoArrowForward className="h-4 w-4 text-muted-foreground transition-transform duration-200 group-open:rotate-90 shrink-0" />
                    </summary>
                    <div className="px-5 pb-5 md:px-6 md:pb-6 pl-[4.25rem] md:pl-[4.75rem]">
                      <p className="text-sm text-muted-foreground leading-relaxed">
                        {faq.a}
                      </p>
                    </div>
                  </details>
                ))}
              </motion.div>
            </div>
          </div>
        </section>

        {/* By the Numbers */}
        <section className="border-b py-16 md:py-24">
          <div className="container mx-auto px-4">
            <div className="mx-auto max-w-5xl">
              <div className="text-center mb-14">
                <Badge variant="secondary" className="mb-4">
                  <IoStatsChart className="h-3 w-3 mr-1" />
                  By the Numbers
                </Badge>
                <h2 className="text-3xl font-bold mb-4">
                  Built for Scale and Security
                </h2>
                <p className="text-lg text-muted-foreground max-w-2xl mx-auto">
                  A comprehensive protocol designed to replace Git for modern
                  teams.
                </p>
              </div>

              <div className="grid gap-px bg-border rounded-xl overflow-hidden border border-border md:grid-cols-3">
                {[
                  {
                    stat: "15",
                    label: "Spec Documents",
                    desc: "A rigorously defined protocol before a single line of code was written.",
                  },
                  {
                    stat: "20+",
                    label: "Atomic Permissions",
                    desc: "Fine-grained control over trees, branches, snapshots, and synchronization.",
                  },
                  {
                    stat: "5",
                    label: "Built-in Roles",
                    desc: "Owner, Admin, Maintainer, Developer, and Viewer hierarchies out of the box.",
                  },
                ].map((s, i) => (
                  <motion.div
                    key={s.label}
                    initial={{ opacity: 0, y: 20 }}
                    whileInView={{ opacity: 1, y: 0 }}
                    viewport={{ once: true }}
                    transition={{ duration: 0.4, delay: i * 0.1 }}
                    className="bg-card p-8 flex flex-col items-center text-center"
                  >
                    <div className="text-5xl font-bold text-primary mb-2">
                      {s.stat}
                    </div>
                    <div className="text-lg font-semibold mb-3">{s.label}</div>
                    <p className="text-sm text-muted-foreground">{s.desc}</p>
                  </motion.div>
                ))}
              </div>
            </div>
          </div>
        </section>

        {/* CTA Section */}
        <section className="py-20 md:py-32 relative overflow-hidden">
          <div className="absolute inset-0 bg-primary/[0.03]" />
          <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_center,hsl(var(--primary)/0.08),transparent_70%)]" />
          <div className="container mx-auto px-4 relative">
            <div className="mx-auto max-w-2xl text-center">
              <motion.div
                initial={{ opacity: 0, y: 20 }}
                whileInView={{ opacity: 1, y: 0 }}
                viewport={{ once: true }}
                transition={{ duration: 0.5 }}
                className="space-y-6"
              >
                <div className="inline-flex items-center gap-2 rounded-full border border-primary/20 bg-primary/5 px-4 py-1.5 text-xs font-medium text-primary">
                  <IoFlash className="h-3 w-3" />
                  Open Source · MIT Licensed
                </div>
                <h2 className="text-4xl md:text-5xl font-bold tracking-tight">
                  Ready to ship faster?
                </h2>
                <p className="text-lg text-muted-foreground max-w-lg mx-auto">
                  Join the next generation of version control. One command to
                  install, five minutes to migrate.
                </p>

                <div className="flex flex-col sm:flex-row gap-3 justify-center pt-2">
                  <Button size="lg" className="gap-2 group h-12 px-8" asChild>
                    <Link href="/guides/quick-start">
                      Get Started
                      <IoArrowForward className="h-4 w-4 transition-transform group-hover:translate-x-1" />
                    </Link>
                  </Button>
                  <Button
                    size="lg"
                    variant="outline"
                    className="gap-2 h-12 px-8"
                    asChild
                  >
                    <Link href="https://github.com/worktree">
                      <IoLogoGithub className="h-4 w-4" />
                      Star on GitHub
                    </Link>
                  </Button>
                </div>

                <div className="pt-4">
                  <div className="inline-flex items-center gap-4 rounded-lg bg-muted/60 border border-border px-5 py-3 font-mono text-sm">
                    <span className="text-muted-foreground">$</span>
                    <span>cargo install worktree</span>
                    <button
                      onClick={() =>
                        copyToClipboard("cargo install worktree", "cta-install")
                      }
                      className="text-muted-foreground hover:text-foreground transition-colors ml-2"
                    >
                      {copiedCommand === "cta-install" ? (
                        <IoCheckmark className="h-3.5 w-3.5" />
                      ) : (
                        <IoCopy className="h-3.5 w-3.5" />
                      )}
                    </button>
                  </div>
                </div>
              </motion.div>
            </div>
          </div>
        </section>

        {/* Footer */}
        <footer className="border-t bg-card">
          {/* Main footer content */}
          <div className="container mx-auto px-4 py-14">
            <div className="grid gap-10 md:grid-cols-2 lg:grid-cols-[1.5fr,1fr,1fr,1fr]">
              {/* Brand column */}
              <div className="space-y-4">
                <div className="flex items-center gap-2.5">
                  <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-primary/10">
                    <IoGitNetwork className="h-4 w-4 text-primary" />
                  </div>
                  <span className="text-lg font-bold">W0rkTree</span>
                </div>
                <p className="text-sm text-muted-foreground leading-relaxed max-w-xs">
                  Next-generation version control with multi-tenant trees,
                  automatic tracking, and built-in coordination. Written in
                  Rust.
                </p>
                <div className="flex items-center gap-3 pt-1">
                  <Link
                    href="https://github.com/worktree"
                    className="flex h-8 w-8 items-center justify-center rounded-lg border border-border bg-background text-muted-foreground transition-colors hover:text-foreground hover:border-foreground/20"
                  >
                    <IoLogoGithub className="h-4 w-4" />
                  </Link>
                  <Link
                    href="https://twitter.com/worktree"
                    className="flex h-8 w-8 items-center justify-center rounded-lg border border-border bg-background text-muted-foreground transition-colors hover:text-foreground hover:border-foreground/20"
                  >
                    <IoLogoTwitter className="h-4 w-4" />
                  </Link>
                  <Link
                    href="https://discord.gg/worktree"
                    className="flex h-8 w-8 items-center justify-center rounded-lg border border-border bg-background text-muted-foreground transition-colors hover:text-foreground hover:border-foreground/20"
                  >
                    <IoLogoDiscord className="h-4 w-4" />
                  </Link>
                  <Link
                    href="mailto:hello@worktree.dev"
                    className="flex h-8 w-8 items-center justify-center rounded-lg border border-border bg-background text-muted-foreground transition-colors hover:text-foreground hover:border-foreground/20"
                  >
                    <IoMail className="h-4 w-4" />
                  </Link>
                </div>
              </div>

              {/* Product column */}
              <div>
                <h4 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-4">
                  Product
                </h4>
                <ul className="space-y-2.5">
                  {[
                    { label: "Features", href: "/features" },
                    { label: "Changelog", href: "/changelog" },
                    { label: "Roadmap", href: "/roadmap" },
                    { label: "Security", href: "/security" },
                    {
                      label: "Staged Visibility",
                      href: "/features#staged-visibility",
                    },
                    { label: "Multi-Tenancy", href: "/features#iam" },
                  ].map((link) => (
                    <li key={link.label}>
                      <Link
                        href={link.href}
                        className="text-sm text-muted-foreground transition-colors hover:text-foreground"
                      >
                        {link.label}
                      </Link>
                    </li>
                  ))}
                </ul>
              </div>

              {/* Docs column */}
              <div>
                <h4 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-4">
                  Documentation
                </h4>
                <ul className="space-y-2.5">
                  {[
                    { label: "Quick Start", href: "/guides/quick-start" },
                    { label: "Protocol Spec", href: "/docs/protocol" },
                    { label: "CLI Reference", href: "/docs/cli" },
                    { label: "SDK Reference", href: "/docs/sdk" },
                    { label: "Server Guide", href: "/docs/server" },
                    { label: "Migration Guide", href: "/guides/migration" },
                  ].map((link) => (
                    <li key={link.label}>
                      <Link
                        href={link.href}
                        className="text-sm text-muted-foreground transition-colors hover:text-foreground"
                      >
                        {link.label}
                      </Link>
                    </li>
                  ))}
                </ul>
              </div>

              {/* Community column */}
              <div>
                <h4 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-4">
                  Community
                </h4>
                <ul className="space-y-2.5">
                  {[
                    { label: "GitHub", href: "https://github.com/worktree" },
                    { label: "Discord", href: "https://discord.gg/worktree" },
                    { label: "Articles", href: "/articles" },
                    { label: "Contributing", href: "/contributing" },
                    { label: "Code of Conduct", href: "/code-of-conduct" },
                  ].map((link) => (
                    <li key={link.label}>
                      <Link
                        href={link.href}
                        className="text-sm text-muted-foreground transition-colors hover:text-foreground"
                      >
                        {link.label}
                      </Link>
                    </li>
                  ))}
                </ul>
              </div>
            </div>
          </div>

          {/* Bottom bar */}
          <div className="border-t border-border">
            <div className="container mx-auto px-4 py-5 flex flex-col md:flex-row items-center justify-between gap-4">
              <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                <span>Built with</span>
                <IoHeart className="h-3 w-3 text-primary" />
                <span>by the W0rkTree team</span>
                <span className="mx-1.5">·</span>
                <span>MIT License</span>
              </div>
              <div className="flex items-center gap-4 text-xs text-muted-foreground">
                <Link
                  href="/privacy"
                  className="transition-colors hover:text-foreground"
                >
                  Privacy
                </Link>
                <Link
                  href="/terms"
                  className="transition-colors hover:text-foreground"
                >
                  Terms
                </Link>
                <span>© 2025 W0rkTree</span>
              </div>
            </div>
          </div>
        </footer>
      </main>
    </div>
  );
}
