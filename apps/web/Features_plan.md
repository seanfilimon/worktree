# W0rkTree Website — Complete Page Plan

> **Last updated:** June 2025
> **Stack:** Next.js 15, fumadocs, MDX, Tailwind CSS, GeistMono
> **Repo path:** `apps/web/`

---

## Table of Contents

1. [Current State Audit](#1-current-state-audit)
2. [Resources Section — Existing Pages](#2-resources-section--existing-pages)
3. [Resources Section — New Pages Needed](#3-resources-section--new-pages-needed)
4. [Other Website Pages Needed](#4-other-website-pages-needed)
5. [Page Structure Standards](#5-page-structure-standards)
6. [Priority & Phases](#6-priority--phases)
7. [File Tree Summary](#7-file-tree-summary)

---

## 1. Current State Audit

### Site-wide infrastructure

| Component | Status | Notes |
|-----------|--------|-------|
| Root layout (`app/layout.tsx`) | ✅ Done | GeistMono font, fumadocs `RootProvider`, global metadata |
| Site header (`components/site-header.tsx`) | ✅ Done | Mega-dropdown with Features, Docs, Resources tabs |
| Theme toggle | ✅ Done | Dark/light mode via `next-themes` |
| MDX pipeline (`source.config.ts`) | ✅ Done | Collections: `docs`, `guides`, `articles`, `maintainers` |
| Footer | ✅ Done | Links to Features, Changelog, Roadmap, Security, Discord, etc. |
| Homepage (`/`) | ✅ Done | Hero, install commands, features grid, performance bars, FAQ, testimonials, footer |

### Navigation dropdown structure (site-header.tsx)

| Dropdown | Tabs | Status |
|----------|------|--------|
| **Features** | Core VCS · What's Different · Collaboration · Security · Performance | ✅ Links defined, pages **not yet created** |
| **Docs** | Getting Started · References · Infrastructure | ✅ Links to `/docs/*` and `/guides/*` |
| **Resources** | Articles · Guides · Maintainers | ✅ Links to existing content |

### Pages referenced in footer/nav but NOT yet created

| Route | Linked from | Status |
|-------|-------------|--------|
| `/features` | Footer → Product | ❌ Not created |
| `/changelog` | Footer → Product | ❌ Not created |
| `/roadmap` | Footer → Product | ❌ Not created |
| `/security` | Footer → Product | ❌ Not created |
| `/contributing` | Footer → Community | ❌ Not created |
| `/code-of-conduct` | Footer → Community | ❌ Not created |
| `/protocol` | Hero CTA button | ❌ Not created (redirects to `/docs/protocol`) |

---

## 2. Resources Section — Existing Pages

### Articles (`content/articles/`)

| File | Route | Title | Author | Date |
|------|-------|-------|--------|------|
| `hello-world.mdx` | `/articles/hello-world` | Introducing W0rkTree | W0rkTree Team | 2025-01-15 |
| `performance-benchmarks.mdx` | `/articles/performance-benchmarks` | Performance Benchmarks: W0rkTree vs Git | Sean Filimon | 2025-02-10 |

**Schema** (`source.config.ts`): `author` (required), `date` (required), `image`, `tags[]`, `summary`

### Guides (`content/guides/`)

| File | Route | Title |
|------|-------|-------|
| `index.mdx` | `/guides` | Guides overview |
| `quick-start.mdx` | `/guides/quick-start` | Quick Start |
| `migration.mdx` | `/guides/migration` | Migration from Git |
| `architecture.mdx` | `/guides/architecture` | Architecture Overview |
| `versioning.mdx` | `/guides/versioning` | Versioning Reference |
| `admin/` (6 files) | `/guides/admin/*` | Admin Panel guides |

### Maintainers (`content/maintainers/`)

| File | Route | Title | Role |
|------|-------|-------|------|
| `index.mdx` | `/maintainers` | Maintainers overview | — |
| `sean.mdx` | `/maintainers/sean` | Sean Filimon | Lead Developer & Creator |
| `core-team.mdx` | `/maintainers/core-team` | Core Team | Core Maintainers |

**Schema**: `name`, `role`, `avatar`, `github`, `twitter`, `bio` (all optional)

---

## 3. Resources Section — New Pages Needed

### 3.1 New Articles

| File | Title | Topic | Priority |
|------|-------|-------|----------|
| `why-not-git.mdx` | Why Not Git? | Core positioning — what's wrong with the status quo | 🔴 High |
| `nested-trees-explained.mdx` | Nested Trees Explained | Deep dive into the tree model that defines W0rkTree | 🔴 High |
| `rust-for-vcs.mdx` | Why We Chose Rust for Version Control | Language choice, safety, and performance tradeoffs | 🟡 Medium |
| `auto-tracking-deep-dive.mdx` | How Auto Tracking Works | The background daemon, snapshot rules, debounce | 🟡 Medium |
| `dependency-graph-power.mdx` | The Power of Dependency Graphs in VCS | Cross-tree deps, auto TODOs, build ordering | 🟡 Medium |
| `monorepo-at-scale.mdx` | Monorepos at Scale with W0rkTree | Comparison to git submodules, nx, turborepo | 🟡 Medium |
| `git-compatibility-story.mdx` | Git Compatibility: Import, Export, Bridge | Full story of the two-way Git compat layer | 🟢 Low |
| `security-model.mdx` | W0rkTree's Security Model | Permissions, audit log, branch protection in depth | 🟢 Low |
| `blake3-content-addressing.mdx` | BLAKE3 and Content-Addressable Storage | Why BLAKE3, dedup, integrity guarantees | 🟢 Low |
| `linked-branches-atomic-deploys.mdx` | Linked Branches for Atomic Deploys | Multi-tree atomic merges, coordinated shipping | 🟢 Low |

### 3.2 New Guides

| File | Title | Description | Priority |
|------|-------|-------------|----------|
| `nested-trees.mdx` | Working with Nested Trees | Create, navigate, and manage nested trees | 🔴 High |
| `dependency-management.mdx` | Managing Cross-Tree Dependencies | Set up, visualize, and resolve deps | 🔴 High |
| `ci-cd-integration.mdx` | CI/CD Integration | Build pipelines driven by the dep graph | 🟡 Medium |
| `permissions-and-acl.mdx` | Permissions & Access Control | Configure tree/branch-level ACLs | 🟡 Medium |
| `git-interop.mdx` | Git Interop Guide | Import, export, remote bridge, live mirror | 🟡 Medium |
| `auto-tracking-setup.mdx` | Auto Tracking Setup | Configure the daemon, rules, ignore patterns | 🟡 Medium |
| `linked-branches.mdx` | Linked Branches | Create and merge linked multi-tree branches | 🟢 Low |
| `self-hosting.mdx` | Self-Hosting W0rkTree | Run the server on your own infrastructure | 🟢 Low |

### 3.3 New Maintainer Profiles

| File | Title | Description |
|------|-------|-------------|
| `contributors.mdx` | Community Contributors | Acknowledge active open-source contributors |

> Additional maintainer profiles should be added as the team grows. Each new team member gets their own `content/maintainers/<name>.mdx` file following the profile template.

---

## 4. Other Website Pages Needed

### 4.1 Features Landing Page (`/features`)

The features landing page is the **single most important page** for communicating what makes the W0rkTree protocol stand out. It is NOT a grid of links to sub-pages — it IS the pitch.

**Purpose:** Immediately differentiate W0rkTree from Git and every other VCS. Show the protocol's core innovations in a compelling, visual, scrollable page.

**Route:** `/features`

**Content structure:**

1. **Hero** — Bold headline: *"Version control, redesigned from the protocol up."* Sub-line explains this isn't a Git wrapper. CTA to docs/quick-start.
2. **Trees** — The foundational concept. Covers:
   - Nested tree model (tree isolation, ownership, independent branches)
   - Tree-scoped history and snapshots
   - Content-addressable DAG (BLAKE3)
   - How this replaces Git's flat model and submodules
3. **Branches & Merging** — Built on top of trees:
   - Per-tree branch namespaces
   - Linked branches (atomic multi-tree merges)
   - Three-way merge with cross-tree awareness
   - Branch protection rules
4. **Automation** — What Git makes you do manually:
   - Auto tracking (background daemon, snapshot rules)
   - Auto TODOs from dependency changes
   - Dependency graph with build ordering for CI/CD
5. **Collaboration** — Built-in team workflows:
   - Tree permissions and access control at every level
   - Built-in project management / task attribution
   - Audit log with cryptographic verification
6. **Performance** — The engine:
   - Pure Rust, zero-copy reads, parallel ops
   - Dedup storage (70% less than Git)
   - Benchmark comparisons (10× checkout, sub-ms status)
7. **Git Compatibility** — Not a walled garden:
   - Import from Git, export to Git
   - Remote bridge, live mirror mode
   - Round-trip guarantee
8. **CTA** — "Get Started" / "Read the Protocol"

> **No individual feature sub-pages.** Features like tree isolation, linked branches, auto tracking, permissions, and benchmarks are all sections within the landing page or within their parent category section. The dropdown nav items in `featureTabs` should deep-link to anchors on this page (e.g. `/features#trees`, `/features#branches`, `/features#automation`, `/features#security`, `/features#performance`).

**Implementation:** This should be a React page (`app/features/page.tsx`) with animated sections, not MDX. Use `motion` for scroll-triggered animations consistent with the homepage style.

### 4.2 Changelog Page

| Route | Title | Status |
|-------|-------|--------|
| `/changelog` | Changelog | - [ ] |

**Content requirements:**

- [ ] Reverse-chronological list of releases
- [ ] Each entry: version number, date, summary, breaking changes badge, link to full notes
- [ ] Filter by release channel (Stable / Beta / Nightly)
- [ ] RSS feed support
- [ ] Can be MDX-based (one file per release) or a single auto-generated page

### 4.3 Roadmap Page

| Route | Title | Status |
|-------|-------|--------|
| `/roadmap` | Roadmap | - [ ] |

**Content requirements:**

- [ ] Visual timeline of phases (Phase 0–9 from `WORKTREE_PLAN.md`)
- [ ] Current phase highlighted
- [ ] Status indicators (done / in-progress / planned)
- [ ] Links to relevant docs and guides for completed phases

### 4.4 Community / Discord Page

| Route | Title | Status |
|-------|-------|--------|
| `/community` | Community | - [ ] |

**Content requirements:**

- [ ] Discord invite embed / link
- [ ] GitHub repository link
- [ ] Contribution guidelines summary
- [ ] Community stats (if available)
- [ ] Links to Articles, Guides, Maintainers

### 4.5 Security Page

| Route | Title | Status |
|-------|-------|--------|
| `/security` | Security | - [ ] |

**Content requirements:**

- [ ] Security model overview (tree permissions, branch protection, audit log)
- [ ] Responsible disclosure policy
- [ ] Contact information for security reports
- [ ] Links to relevant sections on `/features`

### 4.6 About Page

| Route | Title | Status |
|-------|-------|--------|
| `/about` | About W0rkTree | - [ ] |

**Content requirements:**

- [ ] Mission statement and vision
- [ ] Brief history / origin story
- [ ] Link to Maintainers page
- [ ] Technical philosophy (from architecture guide)
- [ ] Open-source commitment (MIT License)

### 4.7 Contact Page

| Route | Title | Status |
|-------|-------|--------|
| `/contact` | Contact | - [ ] |

**Content requirements:**

- [ ] General inquiries email (`hello@worktree.dev`)
- [ ] Security reports email
- [ ] Discord link
- [ ] GitHub issues link

### 4.8 Contributing Page

| Route | Title | Status |
|-------|-------|--------|
| `/contributing` | Contributing to W0rkTree | - [ ] |

**Content requirements:**

- [ ] How to set up the dev environment
- [ ] Code style and conventions
- [ ] PR process
- [ ] Issue labeling guide
- [ ] Link to Code of Conduct

### 4.9 Code of Conduct

| Route | Title | Status |
|-------|-------|--------|
| `/code-of-conduct` | Code of Conduct | - [ ] |

### 4.10 Blog vs Articles Distinction

> **Decision: No separate blog.** Articles serve as the blog. The `/articles` route is the blog index. Articles use a richer schema (`author`, `date`, `image`, `tags`, `summary`) that supports blog-style presentation. If a distinction is ever needed later, the collection can be split, but for now a single `articles` collection avoids duplication and keeps the nav clean.

---

## 5. Page Structure Standards

### 5.1 Article Page Template

Every article in `content/articles/*.mdx` must include:

```yaml
---
title: "Article Title"                    # Required — displayed as h1
description: "One-line SEO description"   # Required — meta description
author: "Author Name"                     # Required — displayed in byline
date: "YYYY-MM-DD"                        # Required — ISO date string
image: "/blog/slug.png"                   # Optional — OG image / hero
tags: ["tag1", "tag2"]                    # Optional — for filtering
summary: "2-3 sentence summary."          # Optional — shown on index cards
---
```

**Body structure:**

1. Opening paragraph (hook / context)
2. H2 sections with clear headings
3. Code blocks, diagrams, or tables where appropriate
4. Conclusion / summary section
5. CTA linking to relevant docs or guides

### 5.2 Guide Page Template

Every guide in `content/guides/*.mdx` must include:

```yaml
---
title: "Guide Title"                      # Required
description: "What the reader will learn" # Required
---
```

**Body structure:**

1. Opening paragraph explaining what the guide covers
2. Prerequisites `<Callout>` (if any)
3. Step-by-step `<Steps>` sections
4. Platform-specific `<Tabs>` where needed
5. Code examples with expected output
6. "Next Steps" section with `<Cards>` linking to related guides/docs
7. Help callout (Discord + GitHub Issues links)

### 5.3 Maintainer Profile Template

Every maintainer in `content/maintainers/*.mdx` must include:

```yaml
---
title: "Display Name"                     # Required
description: "One-line bio"               # Required
name: "Full Name"                         # Optional — structured data
role: "Role Title"                        # Optional
avatar: "/maintainers/name.jpg"           # Optional — profile photo
github: "username"                        # Optional
twitter: "username"                       # Optional
bio: "Short bio for cards."               # Optional
---
```

**Body structure:**

1. `## About` — Background and expertise
2. `## Focus Areas` — Bullet list of responsibilities
3. `## Philosophy` — Blockquote with personal philosophy (optional)
4. `## Get in Touch` — Links to GitHub, Twitter, etc.

### 5.4 Features Landing Page Structure

The features page (`app/features/page.tsx`) should be a single React page:

**Section structure:**

1. **Hero section** — Headline, sub-headline, CTA. Immediately communicate that this is a new protocol, not a Git skin.
2. **Trees section** (`#trees`) — Nested tree model, isolation, snapshots, content-addressable DAG. This is the foundational innovation.
3. **Branches section** (`#branches`) — Per-tree namespaces, linked branches, three-way merge, branch protection.
4. **Automation section** (`#automation`) — Auto tracking daemon, auto TODOs, dependency graph, CI/CD build ordering.
5. **Collaboration section** (`#collaboration`) — Permissions, project management, task attribution, audit log.
6. **Performance section** (`#performance`) — Rust engine, dedup storage, benchmark numbers.
7. **Git Compatibility section** (`#git-compat`) — Import/export, remote bridge, live mirror, round-trip guarantee.
8. **CTA section** — "Get Started" and "Read the Protocol" buttons.

Each section should include:
- A clear heading and one-line tagline
- 2–4 feature cards within the section for the sub-features
- Visual element: diagram, animation, or code snippet
- Comparison to Git where relevant

> **Nav dropdown mapping:** The `featureTabs` entries in `site-header.tsx` should link to anchor sections on this page rather than individual pages. Example: "Nested Trees" → `/features#trees`, "Auto Tracking" → `/features#automation`, "Rust Engine" → `/features#performance`.

### 5.5 General Page Metadata

All pages should include:

- [ ] `<title>` via metadata export or frontmatter `title`
- [ ] `<meta name="description">` via metadata or frontmatter `description`
- [ ] Open Graph tags (inherited from root layout, override per-page as needed)
- [ ] Consistent use of `SiteHeader` and footer
- [ ] Breadcrumb navigation for nested pages (guides, docs)

---

## 6. Priority & Phases

### Phase 0 — Critical Foundation (Week 1–2)

> Must-have pages that are already linked from the nav or homepage.

- [x] `/features` — Features landing page (the protocol pitch — Trees, Branches, Automation, Collaboration, Performance, Git Compat)
- [x] Article: `why-not-git` — Core positioning piece
- [x] Article: `nested-trees-explained` — Flagship feature explainer
- [x] Guide: `nested-trees` — Hands-on nested tree tutorial
- [x] Guide: `dependency-management` — Cross-tree deps walkthrough
- [x] Update `featureTabs` hrefs in `site-header.tsx` to point to `/features#trees`, `/features#branches`, etc.

### Phase 1 — Core Content (Weeks 3–4)

> Fill out the Resources section with the most important articles and guides.

- [x] Article: `rust-for-vcs`
- [x] Article: `auto-tracking-deep-dive`
- [x] Article: `dependency-graph-power`
- [x] Article: `monorepo-at-scale`
- [x] Guide: `ci-cd-integration`
- [x] Guide: `permissions-and-acl`
- [x] Guide: `git-interop`
- [x] Guide: `auto-tracking-setup`
- [x] Maintainer: `contributors` — Community contributors page
- [x] `/changelog` — Changelog page
- [x] `/roadmap` — Roadmap page

### Phase 2 — Supporting Content (Weeks 5–6)

> Remaining articles, guides, and secondary pages.

- [x] Article: `git-compatibility-story`
- [x] Article: `security-model`
- [x] Article: `blake3-content-addressing`
- [x] Article: `linked-branches-atomic-deploys`
- [x] Guide: `linked-branches`
- [x] Guide: `self-hosting`
- [x] `/about` — About page
- [x] `/contact` — Contact page
- [x] `/security` — Security policy page

### Phase 3 — Ecosystem (Weeks 7–8)

> Community and contributor-facing pages.

- [x] `/community` — Community / Discord page
- [x] `/contributing` — Contributing guide
- [x] `/code-of-conduct` — Code of Conduct

### Phase 4 — Polish & Ongoing (Weeks 9+)

> Continuous improvements and new content.

- [ ] Add OG images for all articles
- [ ] Add hero illustrations/diagrams to features page sections
- [ ] RSS feed for articles
- [ ] Search integration across all content (fumadocs search)
- [ ] Add new maintainer profiles as team grows
- [ ] Write additional articles as features ship (one per release)
- [ ] Changelog automation (pull from GitHub releases)
- [ ] Analytics integration to track most-visited pages

---

## 7. File Tree Summary

Below is the target file structure for all pages described in this plan. All items are now `✅` created (Phase 4 polish items remain).

```
apps/web/
├── app/
│   ├── layout.tsx                              ✅
│   ├── page.tsx                                ✅  (Homepage)
│   ├── articles/
│   │   ├── layout.tsx                          ✅
│   │   └── [[...slug]]/page.tsx                ✅
│   ├── guides/
│   │   ├── layout.tsx                          ✅
│   │   └── [[...slug]]/page.tsx                ✅
│   ├── maintainers/
│   │   ├── layout.tsx                          ✅
│   │   └── [[...slug]]/page.tsx                ✅
│   ├── docs/                                   ✅
│   ├── features/
│   │   ├── layout.tsx                          ✅
│   │   └── page.tsx                            ✅  Features landing (single page, all sections)
│   ├── changelog/
│   │   ├── layout.tsx                          ✅
│   │   └── page.tsx                            ✅
│   ├── roadmap/
│   │   ├── layout.tsx                          ✅
│   │   └── page.tsx                            ✅
│   ├── about/
│   │   ├── layout.tsx                          ✅
│   │   └── page.tsx                            ✅
│   ├── contact/
│   │   ├── layout.tsx                          ✅
│   │   └── page.tsx                            ✅
│   ├── community/
│   │   ├── layout.tsx                          ✅
│   │   └── page.tsx                            ✅
│   ├── security/
│   │   ├── layout.tsx                          ✅
│   │   └── page.tsx                            ✅
│   ├── contributing/
│   │   ├── layout.tsx                          ✅
│   │   └── page.tsx                            ✅
│   └── code-of-conduct/
│       ├── layout.tsx                          ✅
│       └── page.tsx                            ✅
│
├── content/
│   ├── articles/
│   │   ├── hello-world.mdx                     ✅
│   │   ├── performance-benchmarks.mdx          ✅
│   │   ├── why-not-git.mdx                     ✅
│   │   ├── nested-trees-explained.mdx          ✅
│   │   ├── rust-for-vcs.mdx                    ✅
│   │   ├── auto-tracking-deep-dive.mdx         ✅
│   │   ├── dependency-graph-power.mdx          ✅
│   │   ├── monorepo-at-scale.mdx               ✅
│   │   ├── git-compatibility-story.mdx         ✅
│   │   ├── security-model.mdx                  ✅
│   │   ├── blake3-content-addressing.mdx       ✅
│   │   └── linked-branches-atomic-deploys.mdx  ✅
│   ├── guides/
│   │   ├── index.mdx                           ✅
│   │   ├── meta.json                           ✅
│   │   ├── quick-start.mdx                     ✅
│   │   ├── architecture.mdx                    ✅
│   │   ├── migration.mdx                       ✅
│   │   ├── versioning.mdx                      ✅
│   │   ├── nested-trees.mdx                    ✅
│   │   ├── dependency-management.mdx           ✅
│   │   ├── ci-cd-integration.mdx               ✅
│   │   ├── permissions-and-acl.mdx             ✅
│   │   ├── git-interop.mdx                     ✅
│   │   ├── auto-tracking-setup.mdx             ✅
│   │   ├── linked-branches.mdx                 ✅
│   │   ├── self-hosting.mdx                    ✅
│   │   └── admin/                              ✅  (6 files)
│   ├── maintainers/
│   │   ├── index.mdx                           ✅
│   │   ├── meta.json                           ✅
│   │   ├── sean.mdx                            ✅
│   │   ├── core-team.mdx                       ✅
│   │   └── contributors.mdx                    ✅
│   └── docs/                                   ✅  (existing doc tree)
│
└── components/
    ├── site-header.tsx                         ✅
    ├── mdx.tsx                                 ✅
    ├── theme-provider.tsx                      ✅
    ├── theme-toggle.tsx                        ✅
    └── version-selector.tsx                    ✅
```

---

## Nav Dropdown Update — ✅ Completed

All `featureTabs` hrefs in `site-header.tsx` have been updated to point to anchor sections on the single `/features` page:

| Feature Item | `href` |
|--------------|--------|
| Nested Trees | `/features#trees` |
| Branching & Merging | `/features#branches` |
| Snapshots & History | `/features#trees` |
| Dependency Graph | `/features#automation` |
| Linked Branches | `/features#branches` |
| Auto Tracking | `/features#automation` |
| Built-in PM | `/features#collaboration` |
| Auto TODOs | `/features#automation` |
| CI/CD Integration | `/features#automation` |
| Tree Permissions | `/features#collaboration` |
| Branch Protection | `/features#branches` |
| Audit Log | `/features#collaboration` |
| Rust Engine | `/features#performance` |
| Dedup Storage | `/features#performance` |
| 10× Faster Checkout | `/features#performance` |

Footer updated: Pricing link removed from Product column.

---

## Summary — By the Numbers

| Category | Existing | To Create | Total |
|----------|----------|-----------|-------|
| **Articles** | 2 | 10 ✅ | 12 |
| **Guides** | 10 (incl. admin sub-pages) | 8 ✅ | 18 |
| **Maintainer profiles** | 2 (+1 index) | 1 ✅ | 3 (+1 index) |
| **Features page** | 0 | 1 ✅ | 1 |
| **Product pages** (Changelog, Roadmap) | 0 | 2 ✅ | 2 |
| **Community pages** (About, Contact, Community, Contributing, CoC, Security) | 0 | 6 ✅ | 6 |
| **Total new pages** | — | **28 ✅ all created** | — |