# W0rkTree Website Content Update Plan

> **Purpose**: Align all website content — landing page, features page, docs, guides, articles, and static pages — with the authoritative specifications in `crates/worktree-protocol/specs/` and the spec update plan in `specs_plan.md`.
>
> **Scope**: Every user-facing page in `apps/web/`.
>
> **Source of Truth**: The 15 spec files in `crates/worktree-protocol/specs/` are canonical. The `specs_plan.md` provides architectural context and terminology standards. This plan does NOT re-specify the protocol — it maps specs to website content.

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Terminology & Naming Standardization](#2-terminology--naming-standardization)
3. [Content Gap Analysis](#3-content-gap-analysis)
4. [Landing Page (`/`) Updates](#4-landing-page--updates)
5. [Features Page (`/features`) Overhaul](#5-features-page-features-overhaul)
6. [New Feature Pages to Add](#6-new-feature-pages-to-add)
7. [Feature Pages to Remove or Merge](#7-feature-pages-to-remove-or-merge)
8. [Documentation (`/docs`) Updates](#8-documentation-docs-updates)
9. [Guides (`/guides`) Updates](#9-guides-guides-updates)
10. [Articles (`/articles`) Updates](#10-articles-articles-updates)
11. [Static Pages Updates](#11-static-pages-updates)
12. [Navigation & Header Updates](#12-navigation--header-updates)
13. [Footer Updates](#13-footer-updates)
14. [Broken Link Audit & Fixes](#14-broken-link-audit--fixes)
15. [New Documentation Pages to Create](#15-new-documentation-pages-to-create)
16. [Docs to Remove or Deprecate](#16-docs-to-remove-or-deprecate)
17. [Root `/docs/` vs Web Content Deduplication](#17-root-docs-vs-web-content-deduplication)
18. [Implementation Phases](#18-implementation-phases)
19. [File-by-File Change Matrix](#19-file-by-file-change-matrix)
20. [Open Decisions](#20-open-decisions)

---

## 1. Executive Summary

The W0rkTree specs have been fully written and define a system far richer than what the current website communicates. The website was built when the project was younger and is now **missing 6 major feature categories**, **using inconsistent terminology**, **linking to nonexistent routes**, and **framing W0rkTree as "Git but better" rather than a clean replacement**.

### What Must Change

| Category | Current State | Target State |
|---|---|---|
| **Landing page** | 6 features shown (Trees, Deps, Linked Branches, Auto Tracking, PM, Security) | 10+ features reflecting full spec suite |
| **Features page** | 8 sections, no mention of Staged Visibility, IAM, Tenants, Licensing, Storage, Sync | Complete feature showcase with all 15 spec areas |
| **Documentation** | 31 web docs pages + 7 root docs (4 are TODO stubs) | 40+ web docs pages, root docs deprecated or synced |
| **Feature coverage** | Missing: Staged Visibility, Multi-Tenancy, License Compliance, Declarative Access, Sync Protocol, `.wt/`/`.wt-tree/` config model, Reflog, Revert, Tags/Releases, Archiving, Shallow History, Ignore Patterns, Diff system, Merge Requests | Full coverage of all 15 specs |
| **Terminology** | Mixed: "WorkTree"/"Worktree"/"W0rkTree", "commit"/"snapshot", "repository"/"tree", "server" (ambiguous) | Standardized per §2 below |
| **Positioning** | "Git but better" framing in many places | "Git replacement with Git compatibility as a migration bridge" |
| **Broken links** | 10+ links to nonexistent routes | All links resolve |

### Key Numbers

- **15** spec files to map to website content
- **~20** new documentation/guide pages needed
- **~35** existing pages need content updates
- **6** major feature areas completely missing from the website
- **12+** broken links to fix
- **1** global terminology pass required

---

## 2. Terminology & Naming Standardization

Apply these changes **globally** across every file in `apps/web/`:

| Current (Inconsistent) | Correct | Where Used |
|---|---|---|
| WorkTree, Worktree, worktree (as product name) | **W0rkTree** | All marketing copy, headings, descriptions |
| `worktree` (in code/crate/binary names) | `worktree` (lowercase) | Code blocks, CLI examples, package names |
| `.worktree/` | **`.wt/`** | All references to the root config directory |
| `.worktree/` in child trees | **`.wt-tree/`** | All references to per-tree config |
| "commit" | **"snapshot"** | Everywhere except Git compatibility docs |
| "staging area" / "index" | **(remove entirely)** | Never reference except when contrasting with Git |
| "repository" / "repo" | **"worktree"** or **"tree"** | All copy |
| "clone" | **`wt init --from`** | CLI references |
| "push / pull" (Git sense) | **"sync"** (automatic) + **"push"** (explicit) | All workflow descriptions |
| "the server" (ambiguous) | **"bgprocess"** (local) or **"server"** (remote) | Architecture descriptions |
| `.worktreeignore` | **`.wt/ignore`** | Ignore pattern references |
| `wt permission` | **`wt access`** | CLI references |
| `wt daemon` / `wt daemon start` | **`wt worker start`** | CLI references |
| `wt acl grant` | **`wt access grant`** | CLI examples on features page |

### Product Name Rules

- **Marketing/headings**: "W0rkTree" (with zero)
- **CLI/code**: `worktree`, `wt` (lowercase)
- **Crate names**: `worktree-protocol`, `worktree-server`, etc. (lowercase, hyphenated)
- **Never**: "WorkTree" (capital W capital T with letter O), "Worktree" (capital W lowercase t)

---

## 3. Content Gap Analysis

### Features in Specs vs. Features on Website

| Spec Feature | Spec File | On Landing Page? | On Features Page? | In Docs? | In Guides? | In Articles? |
|---|---|---|---|---|---|---|
| Nested Trees | `tree/Tree.md` | ✅ | ✅ | ✅ | ✅ | ✅ |
| Branches & Merging | `tree/Tree.md` | ✅ | ✅ | ✅ | ✅ Linked branches | ❌ |
| Auto Tracking (BGProcess) | `bgprocess/BgProcess.md` | ✅ | ✅ | ✅ Server/daemon | ✅ | ✅ |
| Dependency Graph | `tree/Tree.md` | ✅ | ✅ | ❌ | ✅ | ✅ |
| **Staged Snapshot Visibility** | `visibility/StagedVisibility.md` | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** |
| **Multi-Tenancy & IAM** | `iam/IAM.md`, `iam/TenantModel.md` | ❌ **MISSING** | ❌ **MISSING** | ❌ (permissions only) | ✅ (permissions-and-acl) | ❌ **MISSING** |
| **Declarative Access Control** | `iam/DeclarativeAccess.md` | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** |
| **License Compliance** | `licensing/LicenseCompliance.md` | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** |
| **Sync Protocol** | `sync/Sync.md` | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** |
| **`.wt/` & `.wt-tree/` Config** | `dot-wt/DotWt.md`, `dot-wt-tree/DotWtTree.md` | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** |
| Storage Architecture | `storage/Storage.md` | Partial (dedup mention) | Partial (dedup mention) | ✅ Server/storage | ❌ | ✅ blake3 |
| Security Model | `security/Security.md` | ✅ (basic) | ✅ (basic) | ❌ | ❌ | ✅ |
| Git Compatibility | `WorkTree.md` §Git | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Tags & Releases** | `tree/Tree.md` §Tags | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** |
| **Branch Protection & Merge Requests** | `WorkTree.md` §3.18 | ❌ **MISSING** | Partial (1 card) | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** |
| **Reflog & Revert** | `WorkTree.md` §3.9–3.10 | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** |
| **Large File Handling** | `WorkTree.md` §3.15, `storage/Storage.md` | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** |
| **Shallow History & Partial Sync** | `WorkTree.md` §3.11 | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** |
| **Ignore Patterns** | `WorkTree.md` §3.8 | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** |
| **Archiving & Export** | `WorkTree.md` §3.13 | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** | ❌ **MISSING** |
| **Diff System** | `WorkTree.md` §3.16 | ❌ **MISSING** | ❌ **MISSING** | ✅ protocol/diff-semantics | ❌ **MISSING** | ❌ **MISSING** |

### Summary: 13 of 20 major features are absent or barely mentioned on the website.

---

## 4. Landing Page (`/`) Updates

**File**: `apps/web/app/page.tsx`

### 4.1 Hero Section (Lines ~65–200)

| Change | Details |
|---|---|
| **Update subtitle** | FROM: "A modern version control system designed for teams. Simple, fast, and Git-compatible." → TO: "The protocol-native Git replacement. Multi-tenant, real-time collaboration, built-in access control — with full Git compatibility as a migration bridge." |
| **Update badge** | FROM: "Next-Generation Version Control Protocol" → TO: "Git Replacement Protocol" or "The Post-Git Protocol" |
| **Fix "View Protocol" link** | FROM: `/protocol` (broken) → TO: `/docs/protocol` |
| **Add W0rkTree name standardization** | Ensure all instances use "W0rkTree" (zero) |
| **Update install commands** | Verify `cargo install worktree` and other install methods are still accurate |

### 4.2 Protocol Architecture Section (Lines ~200–400)

**Current**: 6 pipeline cards (Tree Isolation, Dependency Graph, Linked Branches, Auto Tracking, Built-in PM, Security First)

**Target**: Replace the 6-card pipeline with a **10-card grid** (2 rows of 5, or 3+3+4) that maps to the actual spec categories:

| Card | Badge | Spec Source | CLI Example |
|---|---|---|---|
| **Nested Trees** | Core | `tree/Tree.md` | `wt tree add frontend` |
| **Snapshots & History** | Core | `WorkTree.md` | `wt snapshot -m "feat: add login"` |
| **Staged Visibility** | Collaboration | `visibility/StagedVisibility.md` | `wt status --team` |
| **Auto Tracking** | Automation | `bgprocess/BgProcess.md` | `wt worker start` |
| **Sync Protocol** | Infrastructure | `sync/Sync.md` | `wt push` |
| **Multi-Tenant IAM** | Security | `iam/IAM.md`, `iam/TenantModel.md` | `wt access grant @team read` |
| **License Compliance** | Governance | `licensing/LicenseCompliance.md` | `wt license show` |
| **Branch Protection** | Governance | `WorkTree.md` §3.18 | `wt merge-request create` |
| **Large File Handling** | Performance | `storage/Storage.md` | Native — no LFS needed |
| **Git Compatibility** | Migration | `WorkTree.md` §Git | `wt init --from-git ./repo` |

### 4.3 "Why Choose W0rkTree" Section (Lines ~400–520)

**Current**: Repeats the same 6 features from the pipeline section above.

**Target**: Replace with a **"What Git Gets Wrong"** comparison grid (sourced from `specs_plan.md` §2):

| Category | Git Problem | W0rkTree Answer |
|---|---|---|
| UX | Too many ways to do the same thing | One clear way per operation |
| Conceptual | Staging area is confusing and unnecessary | No staging area — changes tracked automatically |
| Destructive | `git reset --hard` loses work | Snapshots are immutable, append-only |
| Collaboration | Work invisible until push | Staged snapshots give real-time team visibility |
| Security | No built-in access control | Native IAM with tenants, teams, roles, policies |
| Protocol | `git://` has no auth or encryption | TLS 1.3 + QUIC everywhere, mandatory auth |
| Files | Large files require separate LFS | Native chunked storage, lazy loading |
| Legal | Zero license enforcement | Per-path SPDX compliance, server-enforced |

### 4.4 Performance Section (Lines ~520–620)

**Current**: 4 benchmarks (Checkout, Status, Dep Resolution, Storage)

**Target**: Keep existing benchmarks but add a footnote acknowledging these are projected benchmarks for the alpha. Add a 5th benchmark row:

| Metric | W0rkTree | Git | Improvement |
|---|---|---|---|
| Large File Sync | Chunk dedup | Re-upload entire file | ~80% bandwidth savings |

### 4.5 FAQ Section (Lines ~620–850)

**Current**: 5 questions

**Target**: Expand to 8 questions. Keep existing 5 and add:

| New Question | Answer Summary | Source Spec |
|---|---|---|
| "What are staged snapshots?" | Your in-progress work is visible to the team before you push — like a live activity feed for code. Staged ≠ pushed. | `visibility/StagedVisibility.md` |
| "How does multi-tenancy work?" | Every user/org is a tenant with username + email. Tenants own worktrees and grant access via declarative TOML config. Private by default. | `iam/TenantModel.md` |
| "Does W0rkTree handle licensing?" | Yes — per-path SPDX licenses, enforced at the protocol level. The server blocks unauthorized export/fork/sync of proprietary code. | `licensing/LicenseCompliance.md` |

### 4.6 Testimonials Section (Lines ~850–930)

**Action**: Remove or replace. The current testimonials use obviously fictional names (John Doe/StreamlineHQ, Sarah Kim/NovaBuild, Michael Park/ScaleForge). Options:

1. **Remove entirely** until real testimonials are available
2. **Replace with a "Built With" section** showing technology highlights (Rust, BLAKE3, QUIC, TOML config)
3. **Replace with a "By the Numbers" section** showing spec metrics (15 spec documents, 20+ atomic permissions, 5 built-in roles, etc.)

**Recommendation**: Option 3 — replace with a metrics/highlights section that communicates scope without fabricating social proof.

### 4.7 Footer Section (Lines ~980–1198)

See [§13 Footer Updates](#13-footer-updates) for detailed changes.

---

## 5. Features Page (`/features`) Overhaul

**File**: `apps/web/app/features/page.tsx`

The features page is the **single most important sales page**. It currently has 8 sections but is missing the majority of W0rkTree's innovations. This needs a near-complete rewrite.

### 5.1 Current vs. Target Section Map

| # | Current Section | Keep? | Changes |
|---|---|---|---|
| 1 | Hero | ✅ Keep | Update copy to emphasize "replacement, not wrapper" |
| 2 | Nested Trees | ✅ Keep | Update CLI examples (`wt tree add` not `wt tree create`), add `.wt-tree/` mention |
| 3 | Branches & Merging | ✅ Keep | Add Merge Requests, update CLI examples, add branch protection details |
| 4 | Automation | 🔄 Rework | Rename to "Background Process & Automation", reference `wt worker` not `wt daemon`, add auto-snapshot triggers |
| 5 | Collaboration | 🔄 Major rework | Split into "Staged Visibility" + "Team Collaboration" |
| 6 | Performance | ✅ Keep | Add storage architecture details, BLAKE3 mention |
| 7 | Git Compatibility | ✅ Keep | Add Live Mirror Mode details, license compliance interaction |
| 8 | CTA | ✅ Keep | Update links |
| — | **NEW: Staged Snapshot Visibility** | ➕ Add | Entirely new section — W0rkTree's flagship innovation |
| — | **NEW: Multi-Tenancy & IAM** | ➕ Add | Entirely new section |
| — | **NEW: Declarative Access Control** | ➕ Add | Entirely new section |
| — | **NEW: License Compliance** | ➕ Add | Entirely new section |
| — | **NEW: Sync Protocol** | ➕ Add | Entirely new section |
| — | **NEW: Configuration Model** | ➕ Add | `.wt/` and `.wt-tree/` |
| — | **NEW: Large File Handling** | ➕ Add | Entirely new section |
| — | **NEW: Safety & Recovery** | ➕ Add | Reflog, Revert, append-only history |

### 5.2 Target Section Order (14 sections)

The features page should flow from "what you see" to "what's under the hood":

#### Section 1: Hero
- Badge: "The W0rkTree Protocol"
- Headline: "Version control, rebuilt from the protocol up."
- Subline: "Not a Git wrapper. Not a Git extension. A complete replacement with full Git compatibility as a migration bridge."
- CTAs: "Get Started" → `/guides/quick-start`, "Read the Specs" → `/docs/protocol`

#### Section 2: Staged Snapshot Visibility (**NEW — Lead with the flagship**)
- **Spec source**: `visibility/StagedVisibility.md`
- **Anchor**: `#staged-visibility`
- **Badge**: "Real-Time Collaboration"
- **Headline**: "See what your team is working on — before they push."
- **3 cards**:
  - **Live Activity Feed**: Staged snapshots sync automatically. See who's editing what, on which branch, in which tree.
  - **Early Conflict Detection**: "Alice also has staged changes to `auth.rs`" — advisory warnings before merge conflicts happen.
  - **Privacy Controls**: Opt out per tree, pause staging, private branches. Metadata visible; file contents respect license restrictions.
- **CLI example**: `wt status --team` / `wt staged --user alice`
- **Key differentiator callout**: "Git provides zero in-flight visibility. Teams resort to standups and Slack to ask 'what are you working on?' W0rkTree answers this at the protocol level."

#### Section 3: Nested Trees (existing, updated)
- **Spec source**: `tree/Tree.md`
- **Anchor**: `#trees`
- **3 cards**:
  - **Tree Isolation**: Independent versioning, branches, access rules, licensing per tree.
  - **Snapshots & History**: BLAKE3 content-addressed, immutable, append-only DAG.
  - **Replaces Submodules**: No more `git submodule` pain. Trees are first-class, not bolted on.
- **CLI example**: `wt tree add frontend --branch-strategy feature-branch`
- **Add**: Mention `.wt-tree/` config per tree.

#### Section 4: Branches, Merging & Protection (existing, expanded)
- **Spec source**: `tree/Tree.md` §Branches, `WorkTree.md` §3.14 Merge, §3.18 Branch Protection
- **Anchor**: `#branches`
- **4 cards** (was 3):
  - **Per-Tree Namespaces**: Branches scoped to trees, not global.
  - **Linked Branches**: Atomic multi-tree features that must merge together.
  - **Branch Protection**: `no_direct_push`, required reviews, CI gates, signature requirements.
  - **Merge Requests** (**NEW**): Full lifecycle — open → review → approve → merge. Stale review handling. CI integration.
- **CLI example**: `wt merge-request create --source feat/login --target main`

#### Section 5: Multi-Tenancy & IAM (**NEW**)
- **Spec source**: `iam/IAM.md`, `iam/TenantModel.md`, `iam/DeclarativeAccess.md`
- **Anchor**: `#iam`
- **Badge**: "Enterprise-Grade Access Control"
- **Headline**: "Built-in identity, access control, and multi-tenancy."
- **4 cards**:
  - **Tenant Model**: Users and orgs as first-class tenants with username + verified email. Personal and organization types.
  - **5 Built-in Roles**: Owner → Admin → Maintainer → Developer → Viewer. Superset hierarchy.
  - **20+ Atomic Permissions**: Tree, Branch, Snapshot, Sync, Management, Admin scopes. RBAC + ABAC.
  - **Worktree Visibility**: Private (default), Shared (explicit grants), Public (all can read, license governs copying).
- **CLI example**: `wt access grant @frontend-team write --tree frontend`
- **Key differentiator callout**: "Git has zero built-in identity or access control. GitHub/GitLab provide these externally. W0rkTree enforces them at the protocol level."

#### Section 6: Declarative Access Control (**NEW**)
- **Spec source**: `iam/DeclarativeAccess.md`, `dot-wt/DotWt.md` §access/
- **Anchor**: `#access`
- **Badge**: "Terraform-Style Config"
- **Headline**: "Access control as version-controlled TOML files."
- **3 cards**:
  - **Explicit Path Registration**: Every path referenced in a policy must be registered in `config.toml`. No globs. Predictable, auditable, O(1).
  - **Declarative Policies**: Define roles in `roles.toml`, policies in `policies.toml`. Version-controlled, synced, server-enforced.
  - **Scope Hierarchy**: Global → Tenant → Tree → Branch → RegisteredPath. Deny always beats allow at same level.
- **Code example**: Show a `policies.toml` snippet with a team grant on a registered path.

#### Section 7: License Compliance (**NEW**)
- **Spec source**: `licensing/LicenseCompliance.md`
- **Anchor**: `#licensing`
- **Badge**: "Protocol-Level Enforcement"
- **Headline**: "Per-path license compliance, enforced by the server."
- **3 cards**:
  - **SPDX Licenses**: Assign licenses per path — MIT, Apache-2.0, GPL-3.0, proprietary.
  - **License Grants**: Grant cross-tenant access at 3 levels — read-only, modify, redistribute.
  - **Server Enforcement**: Server blocks unauthorized export, fork, sync, archive of proprietary code.
- **CLI example**: `wt license show` / `wt archive --license-report`
- **Key differentiator callout**: "Git has zero license enforcement — it's entirely a legal/honor system. W0rkTree prevents license violations at the protocol level before they happen."

#### Section 8: Background Process & Automation (existing, reworked)
- **Spec source**: `bgprocess/BgProcess.md`
- **Anchor**: `#automation`
- **Badge**: "Automatic by Default"
- **Headline**: "A local daemon that handles the tedium."
- **4 cards** (was 3):
  - **Auto-Snapshots**: Configurable triggers — inactivity timeout, file count, byte threshold, branch switch.
  - **Filesystem Watcher**: Platform-native (inotify/FSEvents/ReadDirectoryChangesW), debounced, ignore-aware.
  - **Auto-Merge**: Non-conflicting remote changes merged automatically on pull.
  - **Crash Recovery** (**NEW**): Journal-based recovery, PID file detection, fsck on startup.
- **CLI example**: `wt worker start` / `wt worker status`

#### Section 9: Sync Protocol (**NEW**)
- **Spec source**: `sync/Sync.md`
- **Anchor**: `#sync`
- **Badge**: "Always Converging"
- **Headline**: "Three operations. Zero confusion."
- **3 cards**:
  - **Staged Sync** (automatic): BGProcess uploads snapshots for team visibility. Not in branch history.
  - **Branch Push** (explicit): `wt push` moves staged work into canonical branch history. Server checks protection rules.
  - **Branch Pull** (automatic): Remote updates sync continuously. No manual `git pull`.
- **Transport details**: QUIC primary (TLS 1.3, 0-RTT reconnect, connection migration), gRPC/HTTP/2 fallback.
- **Offline mode**: All local ops continue. Delta sync on reconnect with exponential backoff.

#### Section 10: Configuration Model (**NEW**)
- **Spec source**: `dot-wt/DotWt.md`, `dot-wt-tree/DotWtTree.md`
- **Anchor**: `#config`
- **Badge**: "Convention Over Configuration"
- **Headline**: "Two folders. Complete control."
- **2 cards**:
  - **`.wt/`** (Root): Project-wide config, access, identity, hooks, reflog, ignore patterns. The ceiling — child trees cannot expand.
  - **`.wt-tree/`** (Per-Tree): Tree-specific overrides. Branch strategy, auto-snapshot intervals, tree-level licenses, additional ignore patterns. Can restrict, never expand.
- **Code example**: Show the `.wt/config.toml` structure with `[worktree]`, `[[registered_path]]`, `[sync]`, `[license]` sections.

#### Section 11: Safety & Recovery (**NEW**)
- **Spec source**: `WorkTree.md` §3.9 Revert, §3.10 Reflog
- **Anchor**: `#safety`
- **Badge**: "Non-Destructive by Design"
- **Headline**: "No rebase. No force push. No lost work."
- **3 cards**:
  - **Append-Only History**: Snapshots are immutable. History is never rewritten. Soft-delete with recovery windows.
  - **Reflog**: Full operation log — snapshots, merges, branch ops, push, pull, revert. Server-synced. Configurable retention.
  - **Revert**: Creates new inverse snapshots. Never modifies history. Supports reverting merge snapshots with parent selection.
- **CLI example**: `wt reflog` / `wt revert <snapshot-id>`

#### Section 12: Large File & Storage (**NEW**)
- **Spec source**: `storage/Storage.md`, `WorkTree.md` §3.15
- **Anchor**: `#storage`
- **Badge**: "No LFS Required"
- **Headline**: "Large files are just files."
- **3 cards**:
  - **Native Chunked Storage**: FastCDC algorithm, content-defined boundaries. Same pipeline for all file sizes.
  - **Lazy Loading**: Stubs locally for unfetched files. Content served on demand via virtual filesystem (FUSE/ProjFS).
  - **Cross-Version Dedup**: Content-addressed chunks are deduplicated across files and versions. ~70% storage savings.
- **Storage architecture note**: BLAKE3 hashing, zstd compression, per-tenant namespacing on server.

#### Section 13: Git Compatibility (existing, updated)
- **Spec source**: `WorkTree.md` §Git Compatibility
- **Anchor**: `#git`
- **3 cards** (keep existing):
  - **Import & Export**: `wt init --from-git`, `wt export-git`
  - **Remote Bridge**: Push/pull to Git remotes
  - **Live Mirror**: Real-time bidirectional sync with Git remotes
- **Add**: Note about license compliance interaction — proprietary paths blocked on Git export, auto-generated LICENSE file, SPDX headers.

#### Section 14: CTA (existing)
- Update "Get Started" link to verified route
- Update "Read the Protocol" link to `/docs/protocol`

### 5.3 Feature Page Navigation

Update `site-header.tsx` feature dropdown items to use anchor links on the features page:

| Nav Item | Target |
|---|---|
| Staged Visibility | `/features#staged-visibility` |
| Nested Trees | `/features#trees` |
| Branches & Merging | `/features#branches` |
| Multi-Tenancy & IAM | `/features#iam` |
| Declarative Access | `/features#access` |
| License Compliance | `/features#licensing` |
| Automation | `/features#automation` |
| Sync Protocol | `/features#sync` |
| Configuration | `/features#config` |
| Safety & Recovery | `/features#safety` |
| Large Files & Storage | `/features#storage` |
| Git Compatibility | `/features#git` |
| Performance | `/features#performance` (keep on landing page) |

---

## 6. New Feature Pages to Add

**Decision from `Features_plan.md`**: No individual feature sub-pages — all features live as sections within `/features`. However, based on the depth of the specs, the following **dedicated deep-dive pages** are warranted as additions to the **guides** section (not feature pages):

| Guide | Route | Spec Source | Priority |
|---|---|---|---|
| Staged Snapshot Visibility | `/guides/staged-visibility` | `visibility/StagedVisibility.md` | **P0 — Flagship feature** |
| Multi-Tenancy & Tenant Model | `/guides/multi-tenancy` | `iam/TenantModel.md` | **P0** |
| Declarative Access Control | `/guides/declarative-access` | `iam/DeclarativeAccess.md` | **P0** |
| License Compliance | `/guides/license-compliance` | `licensing/LicenseCompliance.md` | **P1** |
| The `.wt/` and `.wt-tree/` Config Model | `/guides/config-model` | `dot-wt/DotWt.md`, `dot-wt-tree/DotWtTree.md` | **P1** |
| Sync Protocol | `/guides/sync-protocol` | `sync/Sync.md` | **P1** |
| Tags, Releases & Archiving | `/guides/tags-and-releases` | `tree/Tree.md` §Tags | **P2** |
| Reflog & Recovery | `/guides/reflog-and-recovery` | `WorkTree.md` §3.9–3.10 | **P2** |
| Large File Handling | `/guides/large-files` | `storage/Storage.md`, `WorkTree.md` §3.15 | **P2** |
| Ignore Patterns | `/guides/ignore-patterns` | `WorkTree.md` §3.8 | **P2** |
| Shallow History & Partial Sync | `/guides/shallow-history` | `WorkTree.md` §3.11 | **P3** |
| Merge Strategies & Conflict Resolution | `/guides/merge-strategies` | `WorkTree.md` §3.14 | **P2** |
| Branch Protection & Merge Requests | `/guides/branch-protection` | `WorkTree.md` §3.18 | **P1** |
| Security Model Deep Dive | `/guides/security` | `security/Security.md` | **P2** |

---

## 7. Feature Pages to Remove or Merge

No feature pages need to be removed — the current `/features` page is a single page, not a collection. However, the following content on the features page should be **merged or restructured**:

| Current Content | Action | Reason |
|---|---|---|
| "Collaboration" section (3 cards: Tree Permissions, Built-in PM, Audit Log) | **Split** into "Multi-Tenancy & IAM" + "Staged Visibility" | Current framing is too generic. IAM and Staged Visibility are separate, much larger features. |
| "Auto Tracking" card mentioning `wt daemon start --debounce=2s` | **Update** CLI to `wt worker start` | Terminology standardization per specs |
| "Tree Permissions" card with `wt acl grant @backend-team write --tree=api` | **Update** CLI to `wt access grant @backend-team write --tree api` | Terminology standardization |
| "Built-in PM" card | **Keep but re-scope** | PM features (TODOs, dependency tasks) are real but secondary. Move to "Automation" section. |
| Performance section's `10× checkout, <1ms status, 70% less storage` | **Add context** | Clarify these are protocol-level design targets for the alpha |

---

## 8. Documentation (`/docs`) Updates

### 8.1 Existing Docs to Update

#### CLI Docs (`content/docs/cli/`)

| File | Current | Changes Needed |
|---|---|---|
| `index.mdx` | CLI overview | Add `wt worker`, `wt access`, `wt tenant`, `wt license`, `wt staged`, `wt reflog`, `wt revert`, `wt tag`, `wt release`, `wt archive`, `wt merge-request` command groups |
| `environment.mdx` | Env setup | Add `WT_SYNC_AUTO`, `WT_TRANSPORT`, `WT_LOG_LEVEL`, `WT_DATA_DIR` env vars from specs |
| `git-compatibility.mdx` | Git interop | Add license compliance interaction on export, Live Mirror Mode, `wt init --from-git` |
| `permissions.mdx` | Permission commands | **Major rewrite**: Rename to `access.mdx`, add declarative access model (TOML files), tenant grants, role management, scope hierarchy |
| `repository.mdx` | Repository commands | Rename references from "repository" to "worktree/tree", add `wt init` with `--from-git` and `--shallow` flags |
| `tree-management.mdx` | Tree management | Add `.wt-tree/` config, tree license config, tree access overrides, tree-scoped branch protection |
| `version-control.mdx` | Version control | Add `wt snapshot` (not `wt commit`), `wt push` (explicit), `wt staged` commands, `wt reflog`, `wt revert` |

**New CLI doc pages needed:**

| New File | Content | Spec Source |
|---|---|---|
| `worker.mdx` | `wt worker start/stop/restart/status/logs` commands | `bgprocess/BgProcess.md` |
| `access.mdx` | `wt access grant/revoke/list/test` commands, declarative access | `iam/DeclarativeAccess.md` |
| `tenant.mdx` | `wt tenant create/switch/list/inspect` commands | `iam/TenantModel.md` |
| `license.mdx` | `wt license show/audit` commands | `licensing/LicenseCompliance.md` |
| `staged.mdx` | `wt staged`, `wt staged --user`, `wt staged clear` commands | `visibility/StagedVisibility.md` |
| `tags-releases.mdx` | `wt tag create/list/delete`, `wt release create` commands | `tree/Tree.md` §Tags |
| `merge-request.mdx` | `wt merge-request create/list/approve/merge` commands | `WorkTree.md` §3.18 |
| `archive.mdx` | `wt archive` command with format/license options | `WorkTree.md` §3.13 |

#### Protocol Docs (`content/docs/protocol/`)

| File | Current | Changes Needed |
|---|---|---|
| `index.mdx` | Protocol overview | Add two-runtime model (bgprocess + server), staged vs pushed distinction, IAM protocol |
| `diff-semantics.mdx` | Diff algorithm | Add diff targets, modes, rename detection, filtering, output formats from §3.16 |
| `merge-semantics.mdx` | Merge strategy | Add machine-readable conflict metadata (JSON), binary file handling, merge request integration |
| `object-model.mdx` | Object types | Add Manifest (large file), Delta, Tag, Branch object types from `storage/Storage.md` |
| `snapshot-format.mdx` | Snapshot format | Add revert metadata, tag references, dependency metadata from `tree/Tree.md` |
| `tree-structure.mdx` | Nested tree model | Add `.wt-tree/` config model, tree access control, tree licensing |
| `versioning.mdx` | Protocol versioning | Review for accuracy against `sync/Sync.md` wire format |
| `wire-format.mdx` | Wire/binary format | Add `SyncMessageEnvelope` (magic bytes `b"WT01"`), Bincode serialization, zstd compression |

**New protocol doc pages needed:**

| New File | Content | Spec Source |
|---|---|---|
| `sync-protocol.mdx` | Staged sync, branch push, branch pull, delta sync, offline mode | `sync/Sync.md` |
| `iam-protocol.mdx` | IAM evaluation, scope resolution, policy model, ABAC conditions | `iam/IAM.md` |
| `transport.mdx` | QUIC primary, gRPC fallback, TLS 1.3, connection migration | `sync/Sync.md` §Transport |
| `storage-model.mdx` | Content-addressable store, BLAKE3, chunking, pack files, GC | `storage/Storage.md` |
| `staged-visibility-protocol.mdx` | Staged snapshot data model, WebSocket streaming, retention | `visibility/StagedVisibility.md` |

#### SDK Docs (`content/docs/sdk/`)

| File | Changes Needed |
|---|---|
| `index.mdx` | Add tenant operations, license operations, staged visibility operations, access management |
| `branch-operations.mdx` | Add branch protection, linked branches, merge requests |
| `connecting.mdx` | Add multi-tenant auth, OAuth2 device flow, API key auth |
| `installation.mdx` | Verify package names and versions |
| `permission-operations.mdx` | **Major rewrite**: Rename to `access-operations.mdx`, add declarative access, tenant grants, role management |
| `snapshot-operations.mdx` | Add staged snapshots, revert operations |
| `tree-operations.mdx` | Add tree config (`.wt-tree/`), tree licensing, tree access |

**New SDK doc pages needed:**

| New File | Content |
|---|---|
| `tenant-operations.mdx` | Tenant CRUD, cross-tenant access, visibility modes |
| `license-operations.mdx` | License assignment, grants, compliance checks |
| `staged-operations.mdx` | Staged snapshot queries, real-time subscriptions |

#### Server Docs (`content/docs/server/`)

| File | Changes Needed |
|---|---|
| `index.mdx` | Distinguish server (remote) from bgprocess (local) |
| `architecture.mdx` | Add multi-tenancy, IAM enforcement, license compliance engine, staged snapshot aggregation, merge request system |
| `configuration.mdx` | Add tenant defaults, rate limits, staged retention, audit config from `server/Server.md` |
| `daemon.mdx` | **Clarify**: This is the bgprocess, not the server. Rename to `bgprocess.mdx` or add clear distinction. |
| `deployment.mdx` | Add tenant isolation (logical vs physical), multi-server considerations |
| `event-pipeline.mdx` | Add staged snapshot events, access change events, license check events |
| `installation.mdx` | Verify against current state |
| `monitoring.mdx` | Add Prometheus metrics, health endpoint, audit logging from `server/Server.md` |
| `storage.mdx` | Add per-tenant namespacing, content-addressable details, GC policies from `storage/Storage.md` |

**New server doc pages needed:**

| New File | Content |
|---|---|
| `tenants.mdx` | Tenant management, types, lifecycle, quotas |
| `iam-enforcement.mdx` | How server evaluates IAM policies on every operation |
| `license-enforcement.mdx` | How server enforces license compliance on sync/export/fork |
| `staged-snapshots.mdx` | Server-side staged snapshot storage, aggregation, streaming |
| `merge-requests.mdx` | Server-side merge request lifecycle |
| `api-surface.mdx` | gRPC (sync), REST (admin), WebSocket (real-time) |

### 8.2 Docs Landing Page Update

**File**: `content/docs/index.mdx`

**Changes**:

1. **Update "What is W0rkTree?"** section to include: two-runtime architecture (bgprocess + server), staged snapshot visibility, multi-tenancy, declarative access, license compliance
2. **Update comparison table** — add rows:
   | Feature | Git | W0rkTree |
   |---|---|---|
   | In-flight visibility | None | Staged snapshots |
   | Multi-tenancy | None (platform-level) | Protocol-native |
   | Access control | None (platform-level) | Declarative TOML, RBAC + ABAC |
   | License enforcement | None | Per-path SPDX, server-enforced |
   | Large files | Requires LFS | Native chunked storage |
   | History safety | Rebase, force push | Append-only, no rewrite |
3. **Update "Component Documentation"** cards to include new sections
4. **Add "Key Features" cards** for: Staged Visibility, Multi-Tenancy, Declarative Access, License Compliance

### 8.3 Docs Sidebar Update

**File**: `content/docs/meta.json`

**Target structure**:

```
{
  "title": "Documentation",
  "description": "W0rkTree documentation",
  "pages": [
    "index",
    "---CLI---",
    "...cli",
    "---Protocol---",
    "...protocol",
    "---SDK---",
    "...sdk",
    "---Server---",
    "...server",
    "---Access & IAM---",
    "...iam",
    "---License Compliance---",
    "...licensing"
  ]
}
```

This requires creating two new doc subdirectories:
- `content/docs/iam/` — IAM, tenants, declarative access
- `content/docs/licensing/` — License compliance

---

## 9. Guides (`/guides`) Updates

### 9.1 Existing Guides to Update

| Guide | Changes Needed |
|---|---|
| `quick-start.mdx` | Add `wt worker start` to setup flow, mention staged visibility, update terminology |
| `migration.mdx` | Add license compliance on export, `.wt/` config migration, tenant setup |
| `architecture.mdx` | **Major update**: Add two-runtime model, bgprocess vs server, `.wt/` vs `.wt-tree/`, staged vs pushed |
| `versioning.mdx` | Review for terminology accuracy |
| `auto-tracking-setup.mdx` | Update `wt daemon` → `wt worker`, add auto-snapshot trigger config |
| `ci-cd-integration.mdx` | Add merge request CI gates, branch protection interaction |
| `dependency-management.mdx` | Add linked branches, cross-tree TODO generation |
| `git-interop.mdx` | Add license compliance interaction, Live Mirror Mode |
| `linked-branches.mdx` | Review against `tree/Tree.md` linked branch spec |
| `nested-trees.mdx` | Add `.wt-tree/` config, tree access, tree licensing |
| `permissions-and-acl.mdx` | **Major rewrite**: Add declarative access model, tenant grants, registered paths, scope hierarchy, `.wt/access/` and `.wt-tree/access/` |
| `self-hosting.mdx` | Add multi-tenancy config, tenant isolation, rate limits |

### 9.2 New Guides to Create

| Guide | Route | Spec Source | Priority | Description |
|---|---|---|---|---|
| `staged-visibility.mdx` | `/guides/staged-visibility` | `visibility/StagedVisibility.md` | **P0** | How staged snapshots work, team visibility, `wt status --team`, `wt staged`, privacy controls, retention |
| `multi-tenancy.mdx` | `/guides/multi-tenancy` | `iam/TenantModel.md` | **P0** | Tenant types, creation, cross-tenant access, visibility modes, organization setup |
| `declarative-access.mdx` | `/guides/declarative-access` | `iam/DeclarativeAccess.md` | **P0** | Path registration, `roles.toml`, `policies.toml`, scope hierarchy, examples |
| `license-compliance.mdx` | `/guides/license-compliance` | `licensing/LicenseCompliance.md` | **P1** | SPDX assignment, grants, export handling, audit |
| `config-model.mdx` | `/guides/config-model` | `dot-wt/DotWt.md`, `dot-wt-tree/DotWtTree.md` | **P1** | `.wt/` structure, `.wt-tree/` structure, inheritance, ceiling model |
| `sync-protocol.mdx` | `/guides/sync-protocol` | `sync/Sync.md` | **P1** | Staged sync, push, pull, offline mode, transport |
| `branch-protection.mdx` | `/guides/branch-protection` | `WorkTree.md` §3.18 | **P1** | Protection rules, merge requests, review requirements |
| `tags-and-releases.mdx` | `/guides/tags-and-releases` | `tree/Tree.md` §Tags | **P2** | Tag types, releases, artifacts, changelogs |
| `reflog-and-recovery.mdx` | `/guides/reflog-and-recovery` | `WorkTree.md` §3.9–3.10 | **P2** | Reflog commands, revert, recovery workflows |
| `large-files.mdx` | `/guides/large-files` | `storage/Storage.md` | **P2** | Chunked storage, lazy loading, threshold config, Git LFS interop |
| `ignore-patterns.mdx` | `/guides/ignore-patterns` | `WorkTree.md` §3.8 | **P2** | Ignore hierarchy, `.wt/ignore`, `.wt-tree/ignore`, built-in defaults, migration from `.gitignore` |
| `merge-strategies.mdx` | `/guides/merge-strategies` | `WorkTree.md` §3.14 | **P2** | Three-way merge, conflict markers, machine-readable metadata, resolution CLI |
| `security-deep-dive.mdx` | `/guides/security-deep-dive` | `security/Security.md` | **P2** | Transport security, auth, secret scanning, signing, audit logging, threat model |
| `shallow-history.mdx` | `/guides/shallow-history` | `WorkTree.md` §3.11 | **P3** | Shallow init, lazy history, partial tree sync, depth expansion |
| `archiving.mdx` | `/guides/archiving` | `WorkTree.md` §3.13 | **P3** | Archive formats, license interaction, release integration |

### 9.3 Guides Sidebar Update

**File**: `content/guides/meta.json`

Add new categories/sections to the guide navigation:

- **Getting Started**: quick-start, architecture, config-model
- **Core Workflows**: staged-visibility, sync-protocol, merge-strategies, reflog-and-recovery
- **Trees & Branches**: nested-trees, linked-branches, branch-protection, tags-and-releases, dependency-management
- **Access & Governance**: multi-tenancy, declarative-access, permissions-and-acl, license-compliance
- **Automation**: auto-tracking-setup, ci-cd-integration, ignore-patterns
- **Storage & Performance**: large-files, shallow-history, archiving
- **Security**: security-deep-dive
- **Migration**: migration, git-interop
- **Infrastructure**: self-hosting, admin/

---

## 10. Articles (`/articles`) Updates

### 10.1 Existing Articles to Update

| Article | Changes Needed |
|---|---|
| `hello-world.mdx` | Update to reflect full spec scope — mention staged visibility, multi-tenancy, license compliance |
| `performance-benchmarks.mdx` | Add storage architecture metrics (BLAKE3, FastCDC chunking), clarify benchmark context |
| `why-not-git.mdx` | **Expand significantly** using `specs_plan.md` §2 Git problems table. Add: protocol/security problems, license enforcement gap, multi-tenancy gap. This is the #1 positioning article. |
| `nested-trees-explained.mdx` | Add `.wt-tree/` config, tree access, tree licensing |
| `security-model.mdx` | **Major expansion**: Add threat model (12 threats from `security/Security.md`), transport security (TLS 1.3 + QUIC), secret scanning, snapshot signing, audit logging |
| `auto-tracking-deep-dive.mdx` | Update daemon → worker terminology, add auto-snapshot trigger details from `bgprocess/BgProcess.md` |
| `blake3-content-addressing.mdx` | Add storage architecture details from `storage/Storage.md` |
| `git-compatibility-story.mdx` | Add license compliance interaction on Git export |
| `linked-branches-atomic-deploys.mdx` | Review against spec, add merge request integration |
| `monorepo-at-scale.mdx` | Add partial sync, shallow history, large file handling |
| `dependency-graph-power.mdx` | Add automatic TODO generation from `tree/Tree.md` |
| `rust-for-vcs.mdx` | Review for accuracy |

### 10.2 New Articles to Write

| Article | Topic | Spec Source | Priority |
|---|---|---|---|
| `staged-snapshots-explained.mdx` | W0rkTree's flagship feature explained | `visibility/StagedVisibility.md` | **P0** |
| `declarative-access-control.mdx` | Terraform-style access control for version control | `iam/DeclarativeAccess.md` | **P0** |
| `multi-tenancy-for-code.mdx` | Why version control needs multi-tenancy | `iam/TenantModel.md` | **P1** |
| `license-compliance-in-vcs.mdx` | Protocol-level license enforcement | `licensing/LicenseCompliance.md` | **P1** |
| `two-runtime-architecture.mdx` | BGProcess + Server — why two runtimes | `bgprocess/BgProcess.md`, `server/Server.md` | **P1** |
| `no-more-lfs.mdx` | Native large file handling without Git LFS | `storage/Storage.md` | **P2** |
| `append-only-history.mdx` | Why W0rkTree history can never be rewritten | `WorkTree.md` §3.9–3.10 | **P2** |
| `quic-transport.mdx` | Modern transport protocol for version control | `sync/Sync.md` §Transport | **P3** |
| `config-as-code-access.mdx` | `.wt/access/` — access control that lives in your tree | `iam/DeclarativeAccess.md` | **P2** |

---

## 11. Static Pages Updates

### 11.1 About Page (`/about`)

**File**: `apps/web/app/about/page.tsx`

| Section | Changes |
|---|---|
| Mission/Vision | Add: "W0rkTree is not a Git wrapper. It replaces Git at the protocol level." |
| Technical Philosophy | Add: two-runtime architecture, staged visibility, multi-tenancy, declarative access, license compliance |
| Features list | Expand beyond Trees/Automation to include IAM, Tenancy, Licensing, Staged Visibility |

### 11.2 Security Page (`/security`)

**File**: `apps/web/app/security/page.tsx`

**Major expansion needed.** Current page has 3 cards (Tree Permissions, Branch Protection, Audit Log).

**Add from `security/Security.md`:**
- Transport Security (TLS 1.3 everywhere, QUIC primary)
- Authentication (OAuth2 device flow, API keys, JWT)
- Secret Scanning (pre-snapshot, configurable patterns, block on match)
- Snapshot Signing (Ed25519)
- Data Encryption (in-transit + at-rest)
- IPC Security (unix socket / named pipe, owner-only)
- Threat Model summary (12 threats covered)
- Rate Limiting (per plan)
- Incident Response (automated alerting, token revocation)

### 11.3 Roadmap Page (`/roadmap`)

**File**: `apps/web/app/roadmap/page.tsx`

**Update phases to reflect spec completeness.** Add phases for:
- Staged Snapshot Visibility (implementation)
- Multi-Tenancy & IAM (server-side)
- License Compliance (server-side)
- Declarative Access Control (config parsing + enforcement)
- BGProcess extraction (split from current server crate)
- Sync Protocol (QUIC + gRPC)

### 11.4 Changelog Page (`/changelog`)

**File**: `apps/web/app/changelog/page.tsx`

No structural changes needed. Add entries as features ship. Ensure terminology uses "snapshot" not "commit", "tree" not "repository", etc.

### 11.5 Contributing Page (`/contributing`)

**File**: `apps/web/app/contributing/page.tsx`

**Update prerequisites** to reflect current project structure. Add note about `worktree-bgprocess` crate (once created).

### 11.6 Community Page (`/community`)

No major changes needed. Update channel descriptions if Discord channels change.

### 11.7 Contact Page (`/contact`)

No changes needed.

### 11.8 Code of Conduct Page (`/code-of-conduct`)

No changes needed.

---

## 12. Navigation & Header Updates

**File**: `apps/web/components/site-header.tsx`

### 12.1 Features Dropdown — Complete Restructure

**Current tabs**: Core VCS, What's Different, Collaboration, Security, Performance

**Target tabs** (5 tabs, reorganized to match spec categories):

#### Tab 1: Core Protocol
| Item | Description | Link |
|---|---|---|
| Nested Trees | Independent versioning, branches, access per tree | `/features#trees` |
| Snapshots & History | BLAKE3 content-addressed, immutable, append-only | `/features#trees` |
| Branches & Merging | Per-tree namespaces, linked branches, merge requests | `/features#branches` |
| Sync Protocol | Staged sync, explicit push, automatic pull, QUIC transport | `/features#sync` |

#### Tab 2: Collaboration
| Item | Description | Link |
|---|---|---|
| Staged Visibility | See what your team is working on in real-time | `/features#staged-visibility` |
| Auto Tracking | Background daemon, auto-snapshots, auto-merge | `/features#automation` |
| Branch Protection | Required reviews, CI gates, merge request system | `/features#branches` |
| Dependency Graph | Cross-tree dependencies, automatic TODOs, build ordering | `/features#trees` |

#### Tab 3: Access & Governance
| Item | Description | Link |
|---|---|---|
| Multi-Tenancy | Users, orgs, teams as first-class tenants | `/features#iam` |
| Declarative Access | TOML-based RBAC + ABAC, version-controlled | `/features#access` |
| License Compliance | Per-path SPDX, server-enforced, export control | `/features#licensing` |
| Audit Log | Immutable, append-only, cryptographic verification | `/features#iam` |

#### Tab 4: Infrastructure
| Item | Description | Link |
|---|---|---|
| Two-Runtime Model | BGProcess (local) + Server (remote) | `/guides/architecture` |
| Configuration | `.wt/` root + `.wt-tree/` per tree | `/features#config` |
| Storage Engine | BLAKE3, FastCDC chunking, lazy loading, dedup | `/features#storage` |
| Safety & Recovery | Reflog, revert, append-only — no lost work | `/features#safety` |

#### Tab 5: Performance & Compat
| Item | Description | Link |
|---|---|---|
| Rust Engine | Pure Rust, zero-copy reads, parallel operations | `/features#performance` (landing page) |
| Large Files | Native chunked storage, no LFS required | `/features#storage` |
| Git Import/Export | Full migration tooling, live mirror mode | `/features#git` |
| 10× Faster | Sub-ms status, 10× checkout, 70% less storage | `/features#performance` (landing page) |

### 12.2 Documentation Dropdown — Update

**Current tabs**: Getting Started, References, Infrastructure

**Target tabs**:

#### Tab 1: Getting Started
| Item | Link |
|---|---|
| Quick Start | `/guides/quick-start` |
| Architecture Overview | `/guides/architecture` |
| Configuration Model | `/guides/config-model` |
| Migration from Git | `/guides/migration` |

#### Tab 2: Core Concepts
| Item | Link |
|---|---|
| Staged Visibility | `/guides/staged-visibility` |
| Multi-Tenancy | `/guides/multi-tenancy` |
| Declarative Access | `/guides/declarative-access` |
| License Compliance | `/guides/license-compliance` |

#### Tab 3: References
| Item | Link |
|---|---|
| CLI Reference | `/docs/cli` |
| Protocol Spec | `/docs/protocol` |
| SDK Reference | `/docs/sdk` |
| Server Guide | `/docs/server` |

#### Tab 4: Infrastructure
| Item | Link |
|---|---|
| Server Deployment | `/docs/server/deployment` |
| Admin Panel | `/guides/admin` |
| Self-Hosting | `/guides/self-hosting` |
| Security | `/guides/security-deep-dive` |

### 12.3 Resources Dropdown — Update

Add new articles to the Articles tab as they're written (staged-snapshots-explained, declarative-access-control, etc.)

### 12.4 Fix Links

| Current | Fix |
|---|---|
| "Get Started" → `/docs/quick-start` | → `/guides/quick-start` |
| `/pricing` | Remove from nav (no pricing page exists) or create page |

---

## 13. Footer Updates

**File**: `apps/web/app/page.tsx` (lines ~980–1198)

### 13.1 Product Column

| Current | Target |
|---|---|
| Features → `/features` | ✅ Keep |
| Changelog → `/changelog` | ✅ Keep |
| Roadmap → `/roadmap` | ✅ Keep |
| Security → `/security` | ✅ Keep |
| *(missing)* | **Add**: Staged Visibility → `/features#staged-visibility` |
| *(missing)* | **Add**: Multi-Tenancy → `/features#iam` |

### 13.2 Documentation Column

| Current | Target |
|---|---|
| Quick Start → `/docs/quick-start` | **Fix**: → `/guides/quick-start` |
| Protocol Spec → `/docs/protocol` | ✅ Keep |
| CLI Reference → `/docs/cli` | ✅ Keep |
| API Reference → `/docs/api` | **Fix**: → `/docs/sdk` (or `/guides/admin/api-reference`) |
| Migration Guide → `/docs/migration` | **Fix**: → `/guides/migration` |
| *(missing)* | **Add**: Server Guide → `/docs/server` |

### 13.3 Community Column

| Current | Target |
|---|---|
| GitHub → external | ✅ Keep |
| Discord → external | ✅ Keep |
| Articles → `/articles` | ✅ Keep |
| Contributing → `/contributing` | ✅ Keep |
| Code of Conduct → `/code-of-conduct` | ✅ Keep |

### 13.4 Legal Links (Bottom Bar)

| Current | Target |
|---|---|
| Privacy → `/privacy` | Create `/privacy` page or remove link |
| Terms → `/terms` | Create `/terms` page or remove link |

---

## 14. Broken Link Audit & Fixes

### 14.1 Confirmed Broken Links

| Link | Referenced From | Fix |
|---|---|---|
| `/protocol` | Landing page hero "View Protocol" button | → `/docs/protocol` |
| `/docs/quick-start` | Landing page CTA, features page CTA, header "Get Started" | → `/guides/quick-start` |
| `/pricing` | Site header nav | Remove link or create pricing page |
| `/docs/api` | Landing page footer "API Reference" | → `/docs/sdk` or `/guides/admin/api-reference` |
| `/docs/migration` | Landing page footer "Migration Guide" | → `/guides/migration` |
| `/privacy` | Landing page footer | Create page or remove |
| `/terms` | Landing page footer | Create page or remove |

### 14.2 Links to Verify

These links exist but should be verified against actual content:

| Link | Location | Status |
|---|---|---|
| `/guides/quick-start` | Multiple CTAs | ✅ Exists |
| `/docs/cli` | Nav, footer | ✅ Exists |
| `/docs/protocol` | Nav, footer | ✅ Exists |
| `/docs/sdk` | Nav | ✅ Exists |
| `/docs/server` | Nav | ✅ Exists |
| `/guides/admin` | Nav | ✅ Exists |
| `/guides/architecture` | Nav | ✅ Exists |
| `/guides/migration` | Nav | ✅ Exists |
| `/articles` | Nav, footer | ✅ Exists |
| `/maintainers` | Nav | ✅ Exists (via `apps/web/app/maintainers/`) |
| `/changelog` | Footer | ✅ Exists |
| `/roadmap` | Footer | ✅ Exists |
| `/security` | Footer | ✅ Exists |
| `/contributing` | Footer | ✅ Exists |
| `/code-of-conduct` | Footer | ✅ Exists |
| `/about` | Nav/footer | ✅ Exists |
| `/contact` | Nav/footer | ✅ Exists |
| `/community` | Nav/footer | ✅ Exists |

---

## 15. New Documentation Pages to Create

### 15.1 Complete List of New Web Content Pages

#### New Docs (`content/docs/`)

| Path | Topic | Spec Source | Priority |
|---|---|---|---|
| `docs/cli/worker.mdx` | BGProcess/worker CLI commands | `bgprocess/BgProcess.md` | P0 |
| `docs/cli/access.mdx` | Access management CLI | `iam/DeclarativeAccess.md` | P0 |
| `docs/cli/tenant.mdx` | Tenant management CLI | `iam/TenantModel.md` | P0 |
| `docs/cli/license.mdx` | License CLI commands | `licensing/LicenseCompliance.md` | P1 |
| `docs/cli/staged.mdx` | Staged snapshot CLI | `visibility/StagedVisibility.md` | P0 |
| `docs/cli/tags-releases.mdx` | Tags & releases CLI | `tree/Tree.md` §Tags | P2 |
| `docs/cli/merge-request.mdx` | Merge request CLI | `WorkTree.md` §3.18 | P1 |
| `docs/cli/archive.mdx` | Archive/export CLI | `WorkTree.md` §3.13 | P2 |
| `docs/protocol/sync-protocol.mdx` | Sync protocol spec | `sync/Sync.md` | P1 |
| `docs/protocol/iam-protocol.mdx` | IAM evaluation protocol | `iam/IAM.md` | P1 |
| `docs/protocol/transport.mdx` | Transport (QUIC/gRPC) | `sync/Sync.md` | P2 |
| `docs/protocol/storage-model.mdx` | Storage architecture | `storage/Storage.md` | P2 |
| `docs/protocol/staged-visibility-protocol.mdx` | Staged visibility protocol | `visibility/StagedVisibility.md` | P1 |
| `docs/sdk/tenant-operations.mdx` | Tenant SDK operations | `iam/TenantModel.md` | P2 |
| `docs/sdk/license-operations.mdx` | License SDK operations | `licensing/LicenseCompliance.md` | P2 |
| `docs/sdk/staged-operations.mdx` | Staged snapshot SDK ops | `visibility/StagedVisibility.md` | P2 |
| `docs/server/tenants.mdx` | Tenant management | `server/Server.md`, `iam/TenantModel.md` | P1 |
| `docs/server/iam-enforcement.mdx` | Server IAM enforcement | `server/Server.md`, `iam/IAM.md` | P1 |
| `docs/server/license-enforcement.mdx` | Server license enforcement | `server/Server.md`, `licensing/LicenseCompliance.md` | P1 |
| `docs/server/staged-snapshots.mdx` | Server staged snapshot handling | `server/Server.md`, `visibility/StagedVisibility.md` | P1 |
| `docs/server/merge-requests.mdx` | Server merge request system | `server/Server.md` | P2 |
| `docs/server/api-surface.mdx` | gRPC + REST + WebSocket APIs | `server/Server.md` | P2 |
| `docs/iam/index.mdx` | IAM overview | `iam/IAM.md` | P0 |
| `docs/iam/tenants.mdx` | Tenant model reference | `iam/TenantModel.md` | P0 |
| `docs/iam/roles-permissions.mdx` | Roles & permissions reference | `iam/IAM.md` | P0 |
| `docs/iam/declarative-access.mdx` | Declarative access reference | `iam/DeclarativeAccess.md` | P0 |
| `docs/iam/scope-resolution.mdx` | Scope hierarchy & resolution | `iam/IAM.md` §Scope | P1 |
| `docs/licensing/index.mdx` | License compliance overview | `licensing/LicenseCompliance.md` | P1 |
| `docs/licensing/spdx.mdx` | SPDX license assignment | `licensing/LicenseCompliance.md` | P1 |
| `docs/licensing/grants.mdx` | License grant model | `licensing/LicenseCompliance.md` | P1 |
| `docs/licensing/enforcement.mdx` | Server enforcement rules | `licensing/LicenseCompliance.md` | P2 |

#### New Guides (`content/guides/`)

(See §9.2 for full list — 15 new guides)

#### New Articles (`content/articles/`)

(See §10.2 for full list — 9 new articles)

#### New Static Pages (if needed)

| Page | Route | Priority | Decision Needed |
|---|---|---|---|
| Privacy Policy | `/privacy` | P2 | Real privacy policy or remove footer link |
| Terms of Service | `/terms` | P2 | Real ToS or remove footer link |
| Pricing | `/pricing` | P3 | Remove nav link or create page |

**Total new pages**: ~55 (31 docs + 15 guides + 9 articles)

---

## 16. Docs to Remove or Deprecate

### 16.1 Rename Required

| Current File | New Name | Reason |
|---|---|---|
| `content/docs/cli/permissions.mdx` | `content/docs/cli/access.mdx` | `wt permission` → `wt access` terminology change |
| `content/docs/sdk/permission-operations.mdx` | `content/docs/sdk/access-operations.mdx` | Same |

### 16.2 Pages to Evaluate for Removal

No pages should be removed outright. However:

| Page | Action | Reason |
|---|---|---|
| `content/docs/server/daemon.mdx` | Rename to `bgprocess.mdx` or clarify scope | "Daemon" is ambiguous — is it the bgprocess or the server process? |

---

## 17. Root `/docs/` vs Web Content Deduplication

The project has **two** documentation locations:

1. **`worktree/docs/`** — 7 standalone Markdown files (4 are TODO stubs)
2. **`worktree/apps/web/content/docs/`** — 31+ Fumadocs MDX pages (the website)

### Recommendation

**The web content (`apps/web/content/docs/`) is the source of truth.** The root `docs/` files should be handled as follows:

| Root Doc File | Current State | Action |
|---|---|---|
| `docs/README.md` | Index page with placeholder URLs | **Rewrite** as a pointer to the website docs. Add: "Full documentation at https://worktree.dev/docs" |
| `docs/admin-panel.md` | Fully written (~400 lines) | **Keep as reference** — the web content `guides/admin/` mirrors this. Ensure they stay in sync or add a sync note. |
| `docs/cli-reference.md` | Fully written | **Keep as reference** — web content `docs/cli/` is the canonical version. Add deprecation note pointing to website. |
| `docs/git-compatibility.md` | All TODO stubs | **Delete or fill** from `WorkTree.md` §Git Compatibility |
| `docs/protocol-spec.md` | All TODO stubs | **Delete or fill** from spec files in `crates/worktree-protocol/specs/` |
| `docs/sdk-guide.md` | All TODO stubs | **Delete or fill** from web content `docs/sdk/` |
| `docs/server-architecture.md` | All TODO stubs | **Delete or fill** from `server/Server.md` and `bgprocess/BgProcess.md` |

**Long-term**: Consider auto-generating root `docs/` files from web content, or adding a CI check that they stay in sync.

---

## 18. Implementation Phases

### Phase 0 — Critical Fixes (Week 1)

> Fix broken links and terminology. No new content.

- [ ] Fix all broken links identified in §14.1
- [ ] Global terminology pass: "W0rkTree" (product), "snapshot" (not commit), "tree" (not repo), `wt worker` (not `wt daemon`), `wt access` (not `wt permission`/`wt acl`)
- [ ] Update landing page hero subtitle and badge
- [ ] Update landing page "View Protocol" button link
- [ ] Fix footer links (Quick Start, API Reference, Migration Guide)
- [ ] Remove or replace fake testimonials section
- [ ] Remove `/pricing` from nav (or create placeholder page)

**Files touched**: `page.tsx`, `site-header.tsx`, `features/page.tsx`

### Phase 1 — Flagship Features on Website (Weeks 2–3)

> Add the 3 most important missing features to the website.

- [ ] Add "Staged Snapshot Visibility" section to features page
- [ ] Add "Multi-Tenancy & IAM" section to features page
- [ ] Add "Declarative Access Control" section to features page
- [ ] Update landing page architecture section with 10-card grid
- [ ] Update landing page FAQ with 3 new questions
- [ ] Create guide: `staged-visibility.mdx`
- [ ] Create guide: `multi-tenancy.mdx`
- [ ] Create guide: `declarative-access.mdx`
- [ ] Create article: `staged-snapshots-explained.mdx`
- [ ] Create article: `declarative-access-control.mdx`
- [ ] Create docs: `docs/iam/index.mdx`, `docs/iam/tenants.mdx`, `docs/iam/roles-permissions.mdx`, `docs/iam/declarative-access.mdx`
- [ ] Update site-header.tsx with new nav structure (§12)
- [ ] Update landing page "Why Choose W0rkTree" to "What Git Gets Wrong" comparison

**Files touched**: `features/page.tsx`, `page.tsx`, `site-header.tsx`, 8+ new content files

### Phase 2 — Complete Features Page & Core Docs (Weeks 4–5)

> Fill in remaining features page sections and core documentation.

- [ ] Add "License Compliance" section to features page
- [ ] Add "Sync Protocol" section to features page
- [ ] Add "Configuration Model" section to features page
- [ ] Add "Safety & Recovery" section to features page
- [ ] Add "Large File & Storage" section to features page
- [ ] Create guide: `license-compliance.mdx`
- [ ] Create guide: `config-model.mdx`
- [ ] Create guide: `sync-protocol.mdx`
- [ ] Create guide: `branch-protection.mdx`
- [ ] Create docs: `docs/licensing/index.mdx`, `docs/licensing/spdx.mdx`, `docs/licensing/grants.mdx`
- [ ] Create docs: `docs/cli/worker.mdx`, `docs/cli/access.mdx`, `docs/cli/tenant.mdx`, `docs/cli/staged.mdx`
- [ ] Create article: `multi-tenancy-for-code.mdx`
- [ ] Create article: `license-compliance-in-vcs.mdx`
- [ ] Create article: `two-runtime-architecture.mdx`

**Files touched**: `features/page.tsx`, 15+ new content files

### Phase 3 — Documentation Depth (Weeks 6–7)

> Fill in detailed reference docs and secondary guides.

- [ ] Update all existing CLI docs (§8.1)
- [ ] Update all existing protocol docs (§8.1)
- [ ] Update all existing SDK docs (§8.1)
- [ ] Update all existing server docs (§8.1)
- [ ] Create new protocol docs: `sync-protocol.mdx`, `iam-protocol.mdx`, `transport.mdx`, `storage-model.mdx`, `staged-visibility-protocol.mdx`
- [ ] Create new server docs: `tenants.mdx`, `iam-enforcement.mdx`, `license-enforcement.mdx`, `staged-snapshots.mdx`
- [ ] Create docs: `docs/cli/license.mdx`, `docs/cli/tags-releases.mdx`, `docs/cli/merge-request.mdx`, `docs/cli/archive.mdx`
- [ ] Create docs: `docs/iam/scope-resolution.mdx`
- [ ] Create docs: `docs/licensing/enforcement.mdx`
- [ ] Update docs landing page (`content/docs/index.mdx`)
- [ ] Update docs sidebar (`content/docs/meta.json`)

### Phase 4 — Guides & Articles (Weeks 8–9)

> Complete all guides and articles.

- [ ] Create remaining guides: `tags-and-releases.mdx`, `reflog-and-recovery.mdx`, `large-files.mdx`, `ignore-patterns.mdx`, `merge-strategies.mdx`, `security-deep-dive.mdx`, `shallow-history.mdx`, `archiving.mdx`
- [ ] Create remaining articles: `no-more-lfs.mdx`, `append-only-history.mdx`, `quic-transport.mdx`, `config-as-code-access.mdx`
- [ ] Update all existing guides (§9.1)
- [ ] Update all existing articles (§10.1)
- [ ] Update guides sidebar (`content/guides/meta.json`)

### Phase 5 — Static Pages & Polish (Weeks 10–11)

> Update static pages, handle deduplication, final polish.

- [ ] Update About page (§11.1)
- [ ] Expand Security page (§11.2)
- [ ] Update Roadmap page (§11.3)
- [ ] Update Contributing page (§11.5)
- [ ] Update footer (§13)
- [ ] Handle root `docs/` deduplication (§17)
- [ ] Create Privacy/Terms pages or remove links
- [ ] Final cross-reference audit — all internal links resolve
- [ ] Verify all CLI commands match spec terminology
- [ ] OG images for new articles
- [ ] Review mobile navigation for new dropdown structure

---

## 19. File-by-File Change Matrix

### Existing Files to Modify

| File | Phase | Change Scope | Priority |
|---|---|---|---|
| `apps/web/app/page.tsx` | 0, 1 | Hero, architecture grid, FAQ, testimonials, footer | **P0** |
| `apps/web/app/features/page.tsx` | 1, 2 | Near-complete rewrite — 8 sections → 14 sections | **P0** |
| `apps/web/components/site-header.tsx` | 1 | Restructure all dropdown tabs | **P0** |
| `apps/web/content/docs/index.mdx` | 3 | Update overview, comparison table, feature cards | **P1** |
| `apps/web/content/docs/meta.json` | 3 | Add IAM and Licensing sidebar sections | **P1** |
| `apps/web/content/docs/cli/index.mdx` | 3 | Add new command groups | **P1** |
| `apps/web/content/docs/cli/permissions.mdx` | 3 | Rename + rewrite → `access.mdx` | **P1** |
| `apps/web/content/docs/cli/environment.mdx` | 3 | Add env vars | **P2** |
| `apps/web/content/docs/cli/git-compatibility.mdx` | 3 | Add license interaction | **P2** |
| `apps/web/content/docs/cli/repository.mdx` | 3 | Terminology update | **P2** |
| `apps/web/content/docs/cli/tree-management.mdx` | 3 | Add `.wt-tree/`, access, licensing | **P2** |
| `apps/web/content/docs/cli/version-control.mdx` | 3 | Add staged, reflog, revert | **P2** |
| `apps/web/content/docs/protocol/index.mdx` | 3 | Add two-runtime model | **P2** |
| `apps/web/content/docs/protocol/diff-semantics.mdx` | 3 | Expand from §3.16 | **P3** |
| `apps/web/content/docs/protocol/merge-semantics.mdx` | 3 | Add machine-readable conflict metadata | **P2** |
| `apps/web/content/docs/protocol/object-model.mdx` | 3 | Add Manifest, Delta, Tag types | **P2** |
| `apps/web/content/docs/protocol/snapshot-format.mdx` | 3 | Add revert/tag metadata | **P3** |
| `apps/web/content/docs/protocol/tree-structure.mdx` | 3 | Add `.wt-tree/`, access, licensing | **P2** |
| `apps/web/content/docs/protocol/versioning.mdx` | 3 | Review accuracy | **P3** |
| `apps/web/content/docs/protocol/wire-format.mdx` | 3 | Add SyncMessageEnvelope | **P2** |
| `apps/web/content/docs/sdk/index.mdx` | 3 | Add tenant, license, staged ops | **P2** |
| `apps/web/content/docs/sdk/branch-operations.mdx` | 3 | Add protection, linked, MRs | **P3** |
| `apps/web/content/docs/sdk/connecting.mdx` | 3 | Add multi-tenant auth | **P3** |
| `apps/web/content/docs/sdk/permission-operations.mdx` | 3 | Rename + rewrite → `access-operations.mdx` | **P2** |
| `apps/web/content/docs/sdk/snapshot-operations.mdx` | 3 | Add staged, revert | **P3** |
| `apps/web/content/docs/sdk/tree-operations.mdx` | 3 | Add tree config, licensing, access | **P3** |
| `apps/web/content/docs/server/index.mdx` | 3 | Distinguish server vs bgprocess | **P1** |
| `apps/web/content/docs/server/architecture.mdx` | 3 | Add tenancy, IAM, licensing, staged | **P1** |
| `apps/web/content/docs/server/configuration.mdx` | 3 | Add tenant defaults, rate limits | **P2** |
| `apps/web/content/docs/server/daemon.mdx` | 3 | Rename/clarify as bgprocess | **P2** |
| `apps/web/content/docs/server/deployment.mdx` | 3 | Add tenant isolation | **P2** |
| `apps/web/content/docs/server/event-pipeline.mdx` | 3 | Add staged, access, license events | **P3** |
| `apps/web/content/docs/server/monitoring.mdx` | 3 | Add Prometheus, audit logging | **P3** |
| `apps/web/content/docs/server/storage.mdx` | 3 | Add per-tenant namespacing, GC | **P2** |
| `apps/web/content/guides/quick-start.mdx` | 4 | Add worker, staged visibility | **P1** |
| `apps/web/content/guides/architecture.mdx` | 4 | **Major update**: two-runtime, `.wt/` vs `.wt-tree/` | **P1** |
| `apps/web/content/guides/migration.mdx` | 4 | Add license, tenant, `.wt/` config | **P2** |
| `apps/web/content/guides/auto-tracking-setup.mdx` | 4 | Update daemon → worker | **P1** |
| `apps/web/content/guides/permissions-and-acl.mdx` | 4 | **Major rewrite**: declarative access | **P1** |
| `apps/web/content/guides/nested-trees.mdx` | 4 | Add `.wt-tree/`, access, licensing | **P2** |
| `apps/web/content/guides/self-hosting.mdx` | 4 | Add multi-tenancy, rate limits | **P2** |
| `apps/web/content/guides/ci-cd-integration.mdx` | 4 | Add MR CI gates | **P3** |
| `apps/web/content/guides/linked-branches.mdx` | 4 | Review against spec | **P3** |
| `apps/web/content/guides/dependency-management.mdx` | 4 | Add TODO generation | **P3** |
| `apps/web/content/guides/git-interop.mdx` | 4 | Add license interaction | **P3** |
| `apps/web/content/articles/hello-world.mdx` | 4 | Update scope | **P2** |
| `apps/web/content/articles/why-not-git.mdx` | 4 | **Major expansion** from §2 Git problems | **P1** |
| `apps/web/content/articles/security-model.mdx` | 4 | **Major expansion** from Security.md | **P2** |
| `apps/web/content/articles/nested-trees-explained.mdx` | 4 | Add `.wt-tree/` | **P2** |
| `apps/web/content/articles/auto-tracking-deep-dive.mdx` | 4 | Update terminology | **P2** |
| `apps/web/content/articles/monorepo-at-scale.mdx` | 4 | Add partial sync, large files | **P3** |
| `apps/web/app/about/page.tsx` | 5 | Update vision, add spec features | **P2** |
| `apps/web/app/security/page.tsx` | 5 | **Major expansion** from Security.md | **P2** |
| `apps/web/app/roadmap/page.tsx` | 5 | Update phases for spec features | **P2** |
| `apps/web/app/contributing/page.tsx` | 5 | Update crate references | **P3** |

### New Files to Create

(See §15.1 for the complete list of ~55 new files)

---

## 20. Open Decisions

These decisions should be made before implementation begins:

| # | Decision | Options | Recommendation | Blocks |
|---|---|---|---|---|
| 1 | **Testimonials section** | Remove / Replace with metrics / Replace with tech highlights | Replace with "By the Numbers" metrics section | Phase 0 |
| 2 | **Pricing page** | Create placeholder / Remove nav link / Create real page | Remove nav link for now — add when pricing model exists | Phase 0 |
| 3 | **Privacy & Terms pages** | Create real pages / Remove footer links / Create placeholder | Create minimal placeholder pages | Phase 5 |
| 4 | **Root `/docs/` files** | Keep in sync / Delete stubs / Add deprecation notices | Add deprecation notices pointing to website docs; fill or delete the 4 TODO stubs | Phase 5 |
| 5 | **Performance benchmarks** | Keep current numbers / Add disclaimer / Remove until real benchmarks | Add "projected targets" disclaimer | Phase 0 |
| 6 | **Feature sub-pages** | All on one `/features` page (current plan) / Create individual pages for top 3 | Keep single-page with anchor links per `Features_plan.md` decision | Phase 1 |
| 7 | **Docs sidebar structure** | Flat / Categorized by component / Categorized by concept | Add IAM and Licensing as new top-level sidebar sections | Phase 3 |
| 8 | **Article publishing cadence** | All at once / 2 per week / 1 per phase | 2 per phase to maintain freshness | Phase 4 |

---

## Summary — By the Numbers

| Metric | Count |
|---|---|
| Existing files to modify | ~55 |
| New documentation pages to create | ~31 |
| New guide pages to create | ~15 |
| New articles to write | ~9 |
| New static pages (privacy/terms) | ~2 |
| Total new content files | **~57** |
| Broken links to fix | **12+** |
| Spec features missing from website | **13 of 20** |
| Implementation phases | **5 (11 weeks)** |
| Open decisions | **8** |

---

## Appendix A: Spec-to-Website Mapping

Quick reference for which spec maps to which website location:

| Spec File | Features Page Section | Guide | Doc Section | Article |
|---|---|---|---|---|
| `WorkTree.md` | Multiple sections | `architecture.mdx` | `docs/protocol/` | `hello-world.mdx` |
| `tree/Tree.md` | §3 Trees, §4 Branches | `nested-trees.mdx`, `linked-branches.mdx`, `tags-and-releases.mdx` | `docs/protocol/tree-structure.mdx` | `nested-trees-explained.mdx` |
| `bgprocess/BgProcess.md` | §8 Automation | `auto-tracking-setup.mdx` | `docs/server/daemon.mdx` → `bgprocess.mdx` | `auto-tracking-deep-dive.mdx`, `two-runtime-architecture.mdx` |
| `server/Server.md` | (implicit) | `self-hosting.mdx` | `docs/server/*` | `two-runtime-architecture.mdx` |
| `visibility/StagedVisibility.md` | §2 Staged Visibility | `staged-visibility.mdx` | `docs/protocol/staged-visibility-protocol.mdx`, `docs/server/staged-snapshots.mdx` | `staged-snapshots-explained.mdx` |
| `iam/IAM.md` | §5 Multi-Tenancy | `permissions-and-acl.mdx` | `docs/iam/*` | (none yet) |
| `iam/TenantModel.md` | §5 Multi-Tenancy | `multi-tenancy.mdx` | `docs/iam/tenants.mdx`, `docs/server/tenants.mdx` | `multi-tenancy-for-code.mdx` |
| `iam/DeclarativeAccess.md` | §6 Declarative Access | `declarative-access.mdx` | `docs/iam/declarative-access.mdx` | `declarative-access-control.mdx`, `config-as-code-access.mdx` |
| `licensing/LicenseCompliance.md` | §7 License Compliance | `license-compliance.mdx` | `docs/licensing/*` | `license-compliance-in-vcs.mdx` |
| `sync/Sync.md` | §9 Sync Protocol | `sync-protocol.mdx` | `docs/protocol/sync-protocol.mdx`, `docs/protocol/transport.mdx` | `quic-transport.mdx` |
| `storage/Storage.md` | §12 Large File & Storage | `large-files.mdx` | `docs/protocol/storage-model.mdx`, `docs/server/storage.mdx` | `no-more-lfs.mdx`, `blake3-content-addressing.mdx` |
| `security/Security.md` | (Security page) | `security-deep-dive.mdx` | (implicit in server docs) | `security-model.mdx` |
| `dot-wt/DotWt.md` | §10 Configuration | `config-model.mdx` | (implicit in CLI/protocol docs) | (none) |
| `dot-wt-tree/DotWtTree.md` | §10 Configuration | `config-model.mdx` | (implicit in tree docs) | (none) |

---

## Appendix B: CLI Command Reference for Website

All CLI commands from specs that should appear somewhere on the website:

| Command Group | Commands | Where to Document |
|---|---|---|
| **Init** | `wt init`, `wt init --from-git`, `wt init --shallow` | CLI docs, quick-start guide |
| **Snapshot** | `wt snapshot [-m]`, `wt snapshot restore` | CLI docs, version-control page |
| **Push** | `wt push` | CLI docs, sync-protocol guide |
| **Sync** | `wt sync pause`, `wt sync resume` | CLI docs, sync-protocol guide |
| **Branch** | `wt branch create/switch/list/delete` | CLI docs, tree-management page |
| **Merge** | `wt merge` | CLI docs, merge-strategies guide |
| **Merge Request** | `wt merge-request create/list/approve/merge` | CLI docs, branch-protection guide |
| **Tree** | `wt tree add/list/remove/sync` | CLI docs, nested-trees guide |
| **Worker** | `wt worker start/stop/restart/status/logs` | CLI docs, auto-tracking guide |
| **Status** | `wt status`, `wt status --team` | CLI docs, staged-visibility guide |
| **Staged** | `wt staged`, `wt staged --user`, `wt staged --branch`, `wt staged clear` | CLI docs, staged-visibility guide |
| **Access** | `wt access grant/revoke/list/test` | CLI docs, declarative-access guide |
| **Tenant** | `wt tenant create/switch/list/inspect` | CLI docs, multi-tenancy guide |
| **License** | `wt license show`, `wt license audit` | CLI docs, license-compliance guide |
| **Diff** | `wt diff` | CLI docs, diff-semantics docs |
| **Log** | `wt log`, `wt show <id>` | CLI docs, version-control page |
| **Reflog** | `wt reflog` | CLI docs, reflog-and-recovery guide |
| **Revert** | `wt revert <snapshot-id>` | CLI docs, reflog-and-recovery guide |
| **Tag** | `wt tag create/list/delete` | CLI docs, tags-and-releases guide |
| **Release** | `wt release create` | CLI docs, tags-and-releases guide |
| **Archive** | `wt archive` | CLI docs, archiving guide |
| **Depend** | `wt depend add`, `wt deps graph` | CLI docs, dependency-management guide |
| **TODO** | `wt todo list/claim/complete` | CLI docs, dependency-management guide |
| **Git** | `wt init --from-git`, `wt export-git`, `wt git mirror` | CLI docs, git-interop guide |
| **Identity** | `wt identity generate`, `wt identity register` | CLI docs, security guide |
| **Audit** | `wt audit log` | CLI docs, security guide |
| **Ignore** | `wt ignore add/list/check` | CLI docs, ignore-patterns guide |
| **Remote** | `wt remote add` | CLI docs, git-interop guide |

---

*This plan should be reviewed and the 8 open decisions resolved before implementation begins. Phase 0 (critical fixes) can start immediately as it requires no decisions.*