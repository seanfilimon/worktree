# W0rkTree Protocol Specifications

> **W0rkTree is a complete Git replacement.** It is not a wrapper, not an extension, not a monorepo tool built on top of Git. W0rkTree is its own version control system with its own protocol, its own storage model, and its own runtime architecture. It speaks Git only when necessary — for migration, interop, and backward compatibility — and nothing more.

---

## Table of Contents

- [Why W0rkTree Exists](#why-w0rktree-exists)
- [Git vs W0rkTree — Full Comparison](#git-vs-w0rktree--full-comparison)
- [Core Terminology](#core-terminology)
- [Architecture — Two Runtimes](#architecture--two-runtimes)
  - [worktree-bgprocess (Local)](#worktree-bgprocess-local)
  - [worktree-server (Remote)](#worktree-server-remote)
- [Key Innovations](#key-innovations)
  - [1. Multi-Tenant Architecture](#1-multi-tenant-architecture)
  - [2. Two-Runtime Model](#2-two-runtime-model)
  - [3. Staged Snapshot Visibility](#3-staged-snapshot-visibility)
  - [4. Declarative Access Control](#4-declarative-access-control)
  - [5. File-Level License Compliance](#5-file-level-license-compliance)
  - [6. Nested Trees](#6-nested-trees)
  - [7. Tags and Releases](#7-tags-and-releases)
  - [8. Branch Protection](#8-branch-protection)
  - [9. Native Large File Handling](#9-native-large-file-handling)
  - [10. Configuration Hierarchy](#10-configuration-hierarchy)
- [Architecture Principles](#architecture-principles)
- [Specification Documents](#specification-documents)
  - [Existing Specifications](#existing-specifications)
  - [Planned Specifications](#planned-specifications)
- [Use Cases](#use-cases)
- [Getting Started](#getting-started)

---

## Why W0rkTree Exists

Git was designed in 2005 for a single purpose: tracking the Linux kernel source. Two decades later, every engineering team on the planet uses it — not because Git is good at what they need, but because nothing better existed.

Git's problems are not bugs. They are fundamental design decisions that cannot be fixed without replacing the system:

- **No organization model.** Git has no concept of tenants, teams, or ownership. A repository is a bag of files with a history. Everything else — access control, team structure, visibility — is bolted on externally.
- **Cryptic command surface.** `rebase --onto`, `reset --soft`, `checkout -b`, `cherry-pick` — Git exposes its internal plumbing as the user interface. Every command requires understanding the object model.
- **No real-time collaboration.** You cannot see what your teammates are working on until they push. There is no staged visibility, no work-in-progress awareness, nothing.
- **Destructive by design.** `git rebase`, `git reset --hard`, `git push --force` — Git allows (and encourages) history rewriting. Data loss is one bad command away.
- **No access control.** Git has no built-in permissions model. File-level access, branch protection, path restrictions — all require external tooling.
- **LFS as an afterthought.** Large files require a separate system (Git LFS) with its own server, its own protocol, and its own failure modes.
- **No license enforcement.** There is no mechanism to track, enforce, or audit file-level licensing in Git. Compliance is entirely manual.

W0rkTree does not patch these problems. It replaces the system that causes them.

---

## Git vs W0rkTree — Full Comparison

| Aspect | Git | W0rkTree |
|---|---|---|
| **Architecture** | Monolithic local tool + separate hosting | Two-runtime system: local bgprocess + remote server |
| **Organization** | Single flat repository per project | Multi-tenant trees with nested subtrees |
| **Identity** | Name + email in config (no verification) | Tenant identity: username + verified email, cross-tenant access |
| **Terminology** | commit, repository, push, pull, checkout | snapshot, tree, sync — plain language throughout |
| **Staging** | Explicit staging area (`git add`) required | No staging area, no index — snapshot captures working state |
| **Commands** | 150+ commands, many overloaded (`checkout` does 3 things) | One job per command, no overloading |
| **Branches** | Global namespace, naming conflicts | Tree-scoped branches with independent strategies |
| **Access Control** | None built-in; relies on hosting platform | Declarative `.wt/access/` and `.wt-tree/access/` with TOML config |
| **Merge** | Merge, rebase, cherry-pick, squash — user chooses | Merge only. No rebase. Append-only history. |
| **Large Files** | Requires Git LFS (separate system) | Native chunked storage with lazy loading — no LFS needed |
| **Protocol** | Git protocol (smart/dumb HTTP, SSH) | Native W0rkTree protocol + Git compatibility layer for migration |
| **Collaboration** | No visibility until push | Staged snapshot visibility — team sees WIP in real-time |
| **History** | Rewritable (`rebase`, `reset`, `force-push`) | Append-only, non-destructive — soft deletes with recovery windows |
| **Monitoring** | None built-in | Server-side telemetry, sync health, tenant activity |
| **Deployment** | External CI/CD reads Git events | Server-enforced branch protection, CI gates, release artifacts |
| **License Enforcement** | None | Per-path SPDX license tracking, server-enforced compliance |
| **Automation** | Manual everything | Auto-snapshot, auto-sync by default |
| **Recovery** | `git reflog` (local only, expires) | Full reflog, soft deletes, configurable recovery windows |
| **Configuration** | `.git/config` + global config | Hierarchical: system → user → `.wt/` → `.wt-tree/` → subtree |
| **Multi-tenancy** | Not supported | First-class tenants, cross-tenant sharing, visibility modes |

---

## Core Terminology

W0rkTree uses plain language. If you know what a word means in English, you know what it means in W0rkTree.

| W0rkTree Term | Replaces (Git) | Meaning |
|---|---|---|
| **Snapshot** | Commit | An immutable point-in-time capture of file state |
| **Tree** | Repository | A versioned collection of files with its own history and branches |
| **Sync** | Push / Pull | Bidirectional transfer of snapshots between bgprocess and server |
| **Branch** | Branch | Same concept, but tree-scoped and independently configured |
| **Tag** | Tag | Immutable named reference to a snapshot |
| **Release** | (no equivalent) | A tag with attached build artifacts and metadata |
| **Tenant** | (no equivalent) | A user or organization with verified identity |
| **Staged Snapshot** | (no equivalent) | A snapshot visible to the team but not yet part of branch history |

**Configuration paths:**
- `.wt/` — Root W0rkTree configuration directory (one per root tree)
- `.wt-tree/` — Per-tree configuration directory (one per individual tree)

**What does not exist in W0rkTree:**
- No staging area. No index. No `add` command.
- No rebase. No `reset --hard`. No `force-push`.
- No `checkout` that does three different things.

---

## Architecture — Two Runtimes

W0rkTree is split into two cooperating runtimes. Neither is optional. The local process handles everything on the developer's machine. The server handles everything shared.

```
┌──────────────────────────────────────────────────┐
│                Developer Machine                  │
│                                                   │
│  ┌─────────────────────────────────────────────┐  │
│  │          worktree-bgprocess                 │  │
│  │          (a.k.a. worktree-worker)           │  │
│  │                                             │  │
│  │  • File system watcher                      │  │
│  │  • Auto-snapshot engine                     │  │
│  │  • Local history & branch management        │  │
│  │  • .wt/ and .wt-tree/ management            │  │
│  │  • Sync client (staged snapshot upload)     │  │
│  │  • Platform-native data storage             │  │
│  └──────────────────┬──────────────────────────┘  │
│                     │                             │
└─────────────────────┼─────────────────────────────┘
                      │  W0rkTree Sync Protocol
                      │  (native + Git compat)
┌─────────────────────┼─────────────────────────────┐
│                     │                             │
│  ┌──────────────────┴──────────────────────────┐  │
│  │            worktree-server                  │  │
│  │                                             │  │
│  │  • Canonical history storage                │  │
│  │  • Multi-tenant management                  │  │
│  │  • IAM & access control enforcement         │  │
│  │  • Staged snapshot visibility               │  │
│  │  • Branch protection enforcement            │  │
│  │  • License compliance enforcement           │  │
│  │  • Tag & release management                 │  │
│  │  • CI/CD gate integration                   │  │
│  └─────────────────────────────────────────────┘  │
│                                                   │
│                  Remote Server                     │
└───────────────────────────────────────────────────┘
```

### worktree-bgprocess (Local)

The background process runs continuously on the developer's machine. It is the only process that touches the working directory.

**Responsibilities:**
- **File watching** — Monitors the working directory for changes in real-time.
- **Auto-snapshots** — Automatically creates snapshots as the developer works. No manual `add` or `commit` required (manual snapshots are also supported).
- **Local history** — Maintains the full local snapshot history and branch state.
- **Branch management** — Creates, switches, and merges branches locally.
- **Sync client** — Uploads staged snapshots to the server, downloads remote changes.
- **`.wt/` folder management** — Owns the root configuration directory.
- **Platform-native storage** — Stores all internal data in the platform-appropriate location (e.g., `AppData` on Windows, `~/.local/share` on Linux, `~/Library` on macOS). Nothing is stored inside the working directory except `.wt/` and `.wt-tree/` configuration.

### worktree-server (Remote)

The server is the source of truth. It is multi-tenant by design.

**Responsibilities:**
- **Canonical history** — The server's snapshot history is authoritative. Local histories sync to it.
- **Tenant management** — Users and organizations are first-class entities with verified identity.
- **IAM enforcement** — All access control rules defined in `.wt/access/` and `.wt-tree/access/` are enforced server-side. The bgprocess cannot bypass them.
- **Staged snapshot storage** — Staged snapshots are stored on the server for team visibility before they become part of branch history.
- **Branch protection** — Merge request reviews, required CI checks, signature requirements — all enforced server-side.
- **License compliance** — Per-path SPDX license rules are enforced on every sync. The server rejects snapshots that violate license policy.
- **Tags and releases** — Immutable tags and releases with artifact storage.
- **Multi-protocol support** — Native W0rkTree protocol for full functionality, Git compatibility protocol for migration and interop.

---

## Key Innovations

### 1. Multi-Tenant Architecture

Every entity in W0rkTree belongs to a **tenant**. A tenant is a user or an organization, identified by a unique username and a verified email address.

- **Tenant isolation** — Each tenant's trees, snapshots, and configuration are isolated by default.
- **Cross-tenant access** — Tenants can grant explicit access to other tenants. No implicit sharing.
- **Visibility modes** — Every tree has a visibility setting:
  - **Private** — Only the owning tenant and explicitly granted tenants can see it.
  - **Shared** — Visible to a defined set of tenants (e.g., all tenants in an organization).
  - **Public** — Visible to everyone. Snapshots can be synced by anyone.

### 2. Two-Runtime Model

W0rkTree is not a monolithic server that you SSH into. It is not a local tool that optionally talks to a remote. It is **two runtimes designed to work together**:

- The **bgprocess** handles everything local: file watching, snapshots, branches, sync.
- The **server** handles everything shared: canonical history, access control, compliance, visibility.

Neither runtime duplicates the other's work. The bgprocess never enforces access control. The server never watches files. This separation is a core architectural constraint, not an implementation detail.

### 3. Staged Snapshot Visibility

In Git, your work is invisible to your team until you push. In W0rkTree, **staged snapshots** bridge the gap:

- A staged snapshot is synced to the server but **not yet part of branch history**.
- Your team can see that you are working on a feature, what files you have changed, and how far along you are.
- Staged snapshots do not pollute branch history. They are a separate visibility layer.
- When you are ready, you **finalize** the staged snapshot into the branch — or discard it.

This eliminates the "what is everyone working on?" problem without requiring standups, status updates, or invasive tooling.

### 4. Declarative Access Control

Access control in W0rkTree is defined in configuration files, not in a web UI or an admin panel.

- **`.wt/access/`** — Root-level access rules (apply to all trees).
- **`.wt-tree/access/`** — Tree-level access rules (apply to a specific tree).
- **TOML format** — Human-readable, version-controlled, diff-able.
- **Explicit path registration** — No globs. Every protected path is listed explicitly.
- **Terraform-style** — Declarative, idempotent, auditable. The access state is the configuration state.

Access rules are version-controlled alongside the code they protect. Changes to access rules go through the same review process as code changes.

### 5. File-Level License Compliance

W0rkTree tracks licenses at the **file path level**, not the repository level.

- **SPDX identifiers** — Every path can have an associated SPDX license expression.
- **Server-enforced** — The server validates license compliance on every sync. A snapshot that introduces a file with an incompatible license is rejected.
- **Export control** — License rules can prevent unauthorized copying or export of files across tenants or visibility boundaries.
- **Audit trail** — Full history of license assignments, changes, and compliance checks.

### 6. Nested Trees

A tree can contain other trees. Each nested tree has:

- **Independent versioning** — Its own snapshot history, separate from the parent.
- **Independent branches** — Its own branch structure.
- **Independent access control** — Its own `.wt-tree/access/` rules.
- **Independent sync** — Can sync to a different server or tenant.

Nested trees replace Git submodules, Git subtrees, and monorepo directory conventions — with a single, consistent model.

### 7. Tags and Releases

Tags and releases are **first-class objects**, not afterthoughts.

- **Tags** — Immutable named references to a specific snapshot. Once created, a tag cannot be moved or deleted (only soft-deleted with a recovery window).
- **Releases** — A tag with attached artifacts (binaries, documentation, changelogs). Releases are the unit of distribution in W0rkTree.
- **Server-managed** — Tags and releases are created and stored on the server. The server enforces naming conventions and uniqueness.

### 8. Branch Protection

Branch protection rules are defined declaratively and enforced by the server.

- **Required reviews** — Merge requests must be approved by a specified number of reviewers.
- **Required CI checks** — The server will not allow a merge until all specified CI checks pass.
- **Signature requirements** — Snapshots targeting protected branches must be cryptographically signed.
- **No bypass** — Branch protection is enforced server-side. The bgprocess cannot override it, even for administrators (unless explicitly configured).

### 9. Native Large File Handling

W0rkTree handles large files natively. There is no separate LFS system.

- **Chunked storage** — Large files are split into content-addressable chunks. Deduplication is automatic.
- **Lazy loading** — The bgprocess downloads file content on demand, not eagerly. Cloning a tree does not require downloading every large file.
- **No configuration** — You do not need to configure tracking patterns or install extensions. Every file, regardless of size, goes through the same pipeline.

### 10. Configuration Hierarchy

W0rkTree configuration follows a strict hierarchy with a **permission ceiling model**:

```
system config          (machine-wide defaults)
  └─ user global       (user preferences)
      └─ .wt/          (root tree config)
          └─ .wt-tree/ (individual tree config)
              └─ subtree config (nested tree overrides)
```

**Key rules:**
- Each level can restrict what lower levels are allowed to do. It can never grant more than what the level above allows.
- The server enforces the ceiling. A `.wt-tree/` config cannot override a `.wt/` restriction.
- Configuration is TOML, version-controlled, and auditable.

---

## Architecture Principles

These are not aspirations. They are constraints enforced by the protocol.

### 1. One Job Per Command

Every W0rkTree command does exactly one thing. There is no `checkout` that creates branches, switches branches, and restores files depending on the flags you pass. If a command name describes the action, that is the only action it performs.

### 2. Plain Terminology

If you have to explain what a word means, it is the wrong word. "Snapshot" is clearer than "commit." "Tree" is clearer than "repository." "Sync" is clearer than the push/pull/fetch distinction. W0rkTree uses words that mean what they say.

### 3. Automatic by Default

The bgprocess creates snapshots automatically as you work. It syncs staged snapshots automatically when connected. You can override this — manual mode exists — but the default is automation. The developer's job is to write code, not to babysit version control.

### 4. Append-Only History

There is no rebase. There is no `reset --hard`. There is no `force-push`. History is append-only. If you make a mistake, you create a new snapshot that fixes it. The original mistake remains in history, because that is what happened. History is a record, not a narrative to be edited.

### 5. Non-Destructive Operations

Nothing is permanently deleted immediately. Branches, snapshots, tags — when "deleted," they enter a soft-delete state with a configurable recovery window. A full reflog is maintained server-side. Accidental data loss requires active effort to achieve.

### 6. Real-Time Collaboration

Staged snapshot visibility means your team knows what you are working on without asking. This is not a chat feature or a status page — it is a core protocol feature. The server maintains real-time visibility into active work across all tenants with appropriate access.

### 7. Multi-Protocol Support

W0rkTree speaks its own native protocol for full functionality. It also speaks Git protocol for migration and interop. A Git client can clone a W0rkTree tree (with reduced functionality). A W0rkTree bgprocess can import from a Git repository. The Git compatibility layer is a bridge, not a dependency.

---

## Specification Documents

### Existing Specifications

| Document | Path | Description |
|---|---|---|
| **Core Concepts** | [`WorkTree.md`](./WorkTree.md) | Core W0rkTree concepts, dependency system, linked branches, design philosophy, and comparison with Git monorepos |
| **Tree Structure** | [`tree/Tree.md`](./tree/Tree.md) | Detailed specification for trees, branches, snapshots, cross-tree coordination, and the TODO system |

### Planned Specifications

The following specifications are **to be created** as the protocol documentation expands:

| Document | Path | Description |
|---|---|---|
| **Background Process** | `bgprocess/BgProcess.md` | Full specification for the local background process — file watching, auto-snapshot engine, local storage layout, sync client behavior, `.wt/` management, and platform-specific integration |
| **Server** | `server/Server.md` | Remote multi-tenant server specification — canonical history storage, tenant lifecycle, enforcement architecture, multi-protocol endpoints, and operational requirements |
| **IAM** | `iam/IAM.md` | Complete IAM specification — authentication, authorization, role model, permission resolution, and enforcement boundaries between bgprocess and server |
| **Declarative Access** | `iam/DeclarativeAccess.md` | Terraform-style declarative access model — `.wt/access/` and `.wt-tree/access/` TOML schema, explicit path registration, idempotent application, and audit trail |
| **Tenant Model** | `iam/TenantModel.md` | Tenant identity specification — username and email verification, tenant types (user vs. organization), cross-tenant access grants, and visibility mode enforcement |
| **License Compliance** | `licensing/LicenseCompliance.md` | Per-path license enforcement — SPDX expression syntax, server-side validation rules, export control policies, and compliance audit logging |
| **Sync Protocol** | `sync/Sync.md` | Sync protocol between bgprocess and server — transport layer, snapshot transfer format, conflict resolution, staged snapshot lifecycle, and Git compatibility wire format |
| **Staged Visibility** | `visibility/StagedVisibility.md` | Staged snapshot visibility specification — staging lifecycle, team visibility rules, finalization and discard flows, and real-time notification model |
| **Storage** | `storage/Storage.md` | Storage architecture — content-addressable chunked storage, deduplication, lazy loading, large file handling, local cache management, and server-side storage layout |
| **Security** | `security/Security.md` | Security model — cryptographic snapshot signing, transport encryption, tenant isolation guarantees, recovery window policies, and threat model |
| **`.wt/` Folder** | `dot-wt/DotWt.md` | Root `.wt/` folder specification — directory layout, configuration files, access rules, and relationship to the root tree |
| **`.wt-tree/` Folder** | `dot-wt-tree/DotWtTree.md` | Per-tree `.wt-tree/` folder specification — directory layout, tree-specific configuration, access overrides, and nested tree behavior |

---

## Use Cases

### Microservices Architecture

Each microservice lives in its own tree with independent versioning, branches, and release cycles. Shared libraries are nested trees. Cross-service dependencies are explicit and tracked. Teams own their trees without stepping on each other.

### Multi-Platform Applications

Frontend, backend, mobile, and shared code each occupy separate trees. Linked branches coordinate cross-platform features. A release in one tree can require matching releases in dependent trees. The dependency system prevents partial deployments.

### Enterprise Codebases

Thousands of developers across hundreds of teams. Declarative access control defines who can touch what — at the file path level, version-controlled alongside the code. License compliance prevents unauthorized use of proprietary modules. The server enforces every rule without relying on developer discipline.

### Open-Source with Proprietary Modules

Public trees for open-source code. Private nested trees for proprietary modules. Per-path SPDX licensing ensures open-source files stay open-source and proprietary files stay proprietary. The server blocks any sync that would violate license boundaries.

### Multi-Tenant Collaboration

Multiple organizations collaborate on shared trees with cross-tenant access grants. Each organization maintains its own private trees alongside shared ones. Visibility modes control what is public, what is shared, and what is private — per tree, enforced by the server.

### Monorepo Migration from Git

Import an existing Git repository as a W0rkTree tree. Split it into nested trees at your own pace. The Git compatibility protocol means existing CI/CD pipelines continue working during migration. Once migrated, switch to the native protocol for full functionality.

---

## Getting Started

Read the specifications in this order:

1. **This document** — You are here. Understand the architecture and terminology first.
2. **[`WorkTree.md`](./WorkTree.md)** — Core concepts, dependency system, and the design philosophy in detail.
3. **[`tree/Tree.md`](./tree/Tree.md)** — Deep dive into trees, branches, snapshots, and cross-tree coordination.
4. **Planned specs** — As they are created, follow the reading order implied by the table above: bgprocess → server → IAM → sync → storage.

---

**W0rkTree is not the next version of Git. It is what comes after Git.**