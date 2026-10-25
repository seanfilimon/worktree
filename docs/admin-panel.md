# Admin panel (`apps/admin`)

The admin panel is a single-page web app for browsing and administering a W0rkTree server: tenants,
members, trees, history, files, staged work, merge requests, protections, licenses, audit log and
accounts. It talks only to the server's [REST API and SSE stream](./server-architecture.md#rest-api);
it has no backend of its own.

| | |
|---|---|
| Location | [`apps/admin`](../apps/admin) (npm package `@worktree/admin`) |
| Stack | Vite, React 19, TypeScript, Tailwind CSS v4, shadcn/ui (Radix), lucide icons, Recharts, React Router, TanStack Query |
| Design | Strictly monochrome: black, white and neutral greys in light and dark themes |
| Production URL | `http://<server>:7420/admin/` (served by the Go server via `--admin-dir`) |
| Dev URL | `http://localhost:5173` |

It replaces the former Yew/WASM crate `crates/worktree-admin`.

## Run in development

Start a server first (see [getting started](./getting-started.md#2-start-a-server)), then:

```bash
cd apps/admin
npm install
npm run dev            # http://localhost:5173
```

The Vite dev server proxies `/api`, `/healthz` and `/metrics` to `http://localhost:7420`. To use a
server elsewhere:

```bash
WT_SERVER_URL=http://192.168.1.20:7420 npm run dev
```

```powershell
$env:WT_SERVER_URL = "http://192.168.1.20:7420"; npm run dev
```

The server's CORS policy always allows `http://localhost:5173`.

## Build and serve

```bash
cd apps/admin
npm run build          # type-check + production build into apps/admin/dist (base path /admin/)
cd ../..
worktree-server --data ./wt-data --admin-dir apps/admin/dist
# open http://localhost:7420/admin/
```

The server serves the files under `/admin/` with an `index.html` fallback, so deep links such as
`/admin/trees/acme/platform/history` work on reload. `GET /` redirects to `/admin/`.

| Script | Purpose |
|---|---|
| `npm run dev` | Dev server with hot reload |
| `npm run build` | `tsc -b && vite build` → `dist/` |
| `npm run preview` | Serve the production build locally |
| `npm test` | Unit and component tests (Vitest + Testing Library, jsdom) |
| `npm run lint` | ESLint, zero warnings allowed |

## Signing in

Sign in with any server account (or register, if the server allows self-registration). The panel
calls `POST /api/v1/auth/login` and keeps the returned bearer token in the browser's
`localStorage` (key `wt.admin.token`); **Sign out** calls `POST /api/v1/auth/logout`, which revokes
it. What you see is filtered by the server's authorization: ordinary users see the tenants they
belong to and the trees they can read; admin-only pages require a server admin
(`--admin-password` creates one).

The live activity feed uses `EventSource` on `/api/v1/events`. Because browsers cannot set headers on
`EventSource`, the token is sent as the `access_token` query parameter; the stream reconnects with
backoff when the connection drops.

## Shell

A collapsible sidebar (icons-only when collapsed, a sheet on phones) groups navigation into
Overview, Workspace, Administration (server admins only) and Settings. The header bar shows the
page's breadcrumbs, a live event-stream indicator and the theme toggle (light / dark / system).

Press **⌘K / Ctrl+K** or **/** anywhere to open the command palette: jump to any page, switch theme,
sign out, or search tenants, trees, branches and (for server admins) accounts via `GET /search`.

## Pages

| Route (under `/admin`) | Page | Data |
|---|---|---|
| `/login`, `/register` | Sign in / create account | `POST /auth/login`, `POST /auth/register` |
| `/` | **Dashboard**: KPI cards (admins), your tenants and trees, live activity feed with type filter and pause, events-per-minute chart, recent pushes, HTTP traffic from `/metrics` | `/admin/stats`, `/events/recent`, SSE `/events`, `/metrics` |
| `/tenants` | **Tenants**: search, create an organization | `GET/POST /tenants` |
| `/tenants/:tenant` | **Tenant detail**: trees, members (inline role edit, add, remove), settings (rename; plan for admins) | `/tenants/{t}/*`, `PATCH /tenants/{t}` |
| `/trees` | **Trees**: filter by tenant and visibility, sort, create | `/tenants/{t}/trees` |
| `/trees/:tenant/:tree/overview` | **Overview**: snapshot/file/size/contributor stats, activity chart (7/30/90 days), languages, contributors, latest snapshots, recent events, README | `/stats`, `/events/recent` |
| `…/history` | Timeline per branch; tick two snapshots to compare | `/snapshots` |
| `…/snapshots/:id` | **Snapshot detail** with its diff against the first parent | `/compare?to=` |
| `…/compare?from=&to=` | **Compare** any two branches, tags or snapshots: snapshot list + diff | `/compare` |
| `…/branches` | Create (from any revision) and delete branches; compare with default | `POST/DELETE /branches` |
| `…/tags` | Create and delete tags | `POST/DELETE /tags` |
| `…/files?path=&snapshot=` | **File browser**: fuzzy "go to file", code view with monochrome syntax emphasis and `#L10-L20` line anchors, Markdown and image previews, per-file history | `/snapshots/{id}`, `/blob/…`, `/history?path=` |
| `…/staged` | **Staged WIP** per branch, with conflict warnings for files staged by several people | `/staged`, `/staged/summary`, SSE |
| `…/merge-requests` | Filter by state, search, open a new MR with a live change preview | `/merge-requests`, `/compare` |
| `…/merge-requests/:n` | **MR detail**: overview timeline, Changes (diff), snapshots, reviewers, readiness checklist; approve (`a`), merge (`m`), close | `/merge-requests/{n}/*`, `/compare` |
| `…/protections` | Branch protection rules with a live "matches branches" preview | `/protections` |
| `…/licenses` | License rules with matching-file preview and SPDX suggestions | `/licenses` |
| `…/settings` | Description, visibility, default branch; delete tree (type its name) | `PATCH/DELETE /trees/{t}/{tree}` |
| `/audit` | **Audit log**: tenant/action/actor/since filters, expandable details, CSV export | `/audit` |
| `/accounts` | **Accounts** (admin): create, reset password, grant admin, disable, membership details | `/admin/accounts*` |
| `/system` | **System** (admin): build, uptime, runtime memory/goroutines with live charts, storage, event bus, raw metrics | `/admin/system`, `/metrics` |
| `/settings` | Profile, change password, API tokens, appearance, sign out | `/auth/*` |

The diff viewer (used by snapshot detail, compare and merge requests) has unified and split views,
a file list with change bars, per-file collapse and "viewed" marks. Additions are shaded with a
`+` gutter; deletions are struck through with a `−` gutter, so diffs read without colour.

Write actions are offered only where the API allows them for your role; the server enforces every
permission regardless of what the UI shows.

## Design system

The panel uses [shadcn/ui](https://ui.shadcn.com) primitives vendored in `src/components/ui`
(add more with `npx shadcn@latest add <name>` from `apps/admin`). Every colour token in
`src/index.css` has zero chroma. Status is shown with weight, inversion (filled vs outline badges),
icons, borders and strike-through, never hue, and charts use a greyscale ramp.

## Code layout

```text
apps/admin/src/
  api/              typed REST client, endpoint wrappers, SSE client, query hooks, token storage
  app/              providers, query client, route table
  auth/             auth context and route guard
  components/ui/    shadcn/ui primitives (vendored)
  components/shell/ sidebar, header, breadcrumbs, command palette, user menu
  components/diff/  diff viewer
  components/files/ file tree, code view, previews
  components/...    dashboard, charts, people, mr building blocks
  lib/              formatting, diff parsing, code tokenizer, metrics parser, glob matching
  pages/            one module per page; tree/ holds the tree tabs
  test/             shared test setup (jsdom polyfills), fixtures, fetch mock, renderApp
```

API types in `src/api/types.ts` mirror the JSON tags of
[`services/server-go/internal/domain/types.go`](../services/server-go/internal/domain/types.go);
keep them in sync when the server's types change.
