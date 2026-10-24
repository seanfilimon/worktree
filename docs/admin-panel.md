# Admin panel (`apps/admin`)

The admin panel is a single-page web app for browsing and administering a W0rkTree server: tenants,
members, trees, history, files, staged work, merge requests, protections, licenses, audit log and
accounts. It talks only to the server's [REST API and SSE stream](./server-architecture.md#rest-api);
it has no backend of its own.

| | |
|---|---|
| Location | [`apps/admin`](../apps/admin) (npm package `@worktree/admin`) |
| Stack | Vite, React 19, TypeScript, Tailwind CSS v4, React Router, TanStack Query |
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

## Pages

| Route (under `/admin`) | Page | Data |
|---|---|---|
| `/login`, `/register` | Sign in / create account | `POST /auth/login`, `POST /auth/register` |
| `/` | **Dashboard**: server stats and live activity | `GET /admin/stats`, `GET /healthz`, SSE `/events` |
| `/tenants` | **Tenants**: tenants visible to you; create an organization | `GET/POST /tenants` |
| `/tenants/:tenant` | **Tenant detail**: members and roles, the tenant's trees | `/tenants/{t}/members`, `/tenants/{t}/trees` |
| `/trees` | **Trees**: all readable trees with branch, snapshot, staged and open-MR counts | `/tenants/{t}/trees` |
| `/trees/:tenant/:tree/branches` | Branches with tips and protection flags | `/trees/{t}/{tree}/branches` |
| `/trees/:tenant/:tree/history` | Snapshot history per branch, snapshot details with file list | `/snapshots`, `/snapshots/{id}` |
| `/trees/:tenant/:tree/files` | File browser at any snapshot; raw file view | `/snapshots/{id}`, `/blob/{snapshot}/{path}` |
| `/trees/:tenant/:tree/staged` | **Staged WIP**: who is changing which files on which branch | `/staged`, SSE |
| `/trees/:tenant/:tree/tags` | Tags | `/tags` |
| `/trees/:tenant/:tree/merge-requests` | Merge requests by state; open a new one | `GET/POST /merge-requests` |
| `/trees/:tenant/:tree/merge-requests/:number` | **Merge request detail**: approve, merge, close; explains `409` (needs a local merge) and `review_required` | `/merge-requests/{n}/*` |
| `/trees/:tenant/:tree/protections` | Branch protection rules | `GET/PUT/DELETE /protections` |
| `/trees/:tenant/:tree/licenses` | License rules (path glob, SPDX, grant) | `GET/PUT/DELETE /licenses` |
| `/trees/:tenant/:tree/settings` | Description, visibility, default branch; delete tree | `PATCH/DELETE /trees/{t}/{tree}` |
| `/audit` | **Audit log** with tenant and action filters | `GET /audit` |
| `/accounts` | **Accounts** (server admin): grant/revoke admin, disable/enable | `/admin/accounts` |
| `/settings` | **Settings**: your profile and API tokens (create, revoke) | `/auth/me`, `/auth/tokens` |

Write actions are offered only where the API allows them for your role; the server enforces every
permission regardless of what the UI shows.

## Code layout

```text
apps/admin/src/
  api/          typed REST client, endpoint wrappers, SSE client, query hooks, token storage
  app/          providers, query client, route table
  auth/         auth context and route guard
  components/   layout and UI primitives
  lib/          formatting, glob matching, file-tree helpers, error explanations
  pages/        one module per page; tree/ holds the tree detail tabs
```

API types in `src/api/types.ts` mirror the JSON tags of
[`services/server-go/internal/domain/types.go`](../services/server-go/internal/domain/types.go);
keep them in sync when the server's types change.
