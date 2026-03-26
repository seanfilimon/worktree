# Implementation Details — `worktree-admin`

The `worktree-admin` crate is a **dual-mode admin panel** for the Worktree version control system. It provides both a WebAssembly single-page application (client-side rendering via Yew) and a server-side rendered HTTP API (via Axum + Maud), gated behind feature flags.

---

## Crate Metadata

- **Name:** `worktree-admin`
- **Edition:** 2021
- **Crate types:** `cdylib` (WASM), `rlib` (Rust library)
- **Binary:** `worktree-admin-server` (requires `ssr` feature)
- **Build tool:** Trunk (for WASM SPA)

### Dependencies (CSR — Default)

| Dependency | Version | Purpose |
|-----------|---------|---------|
| `yew` | 0.21 (csr) | Reactive UI framework |
| `yew-router` | 0.18 | Client-side routing |
| `yew-hooks` | 0.3 | Utility hooks |
| `wasm-bindgen` | 0.2 | JS interop |
| `web-sys` | 0.3 | DOM APIs |
| `gloo` / `gloo-net` | 0.11 / 0.5 | HTTP, storage, timers |
| `serde` / `serde_json` | 1 | Serialization |
| `uuid` | 1 (v4, js) | ID generation |
| `chrono` | 0.4 (wasmbind) | Timestamps |
| `wasm-logger` | 0.2 | Logging |

### Dependencies (SSR — `--features ssr`)

| Dependency | Version | Purpose |
|-----------|---------|---------|
| `axum` | 0.7 (macros) | HTTP framework |
| `tokio` | 1 (full) | Async runtime |
| `tower` / `tower-http` | 0.5 | Middleware (CORS, trace, static files) |
| `tracing` / `tracing-subscriber` | 0.1 / 0.3 | Structured logging |

---

## Architecture

```
src/
├── lib.rs          # App root: Route enum, App component, switch(), run_app()
├── main.rs         # Entry point: calls run_app() (CSR) or starts Axum (SSR)
├── types.rs        # Shared data models (ServerStatus, RepositoryInfo, etc.)
├── utils.rs        # Formatting helpers + inline style utilities
├── components/     # 8 reusable Yew components
│   ├── mod.rs
│   ├── navbar.rs   # Sticky top navigation
│   ├── footer.rs   # Footer with links
│   ├── card.rs     # Generic card container
│   ├── badge.rs    # Color-variant badge
│   ├── button.rs   # Styled button
│   ├── stat_card.rs # Dashboard statistic display
│   ├── repo_card.rs # Repository info card
│   └── loading.rs  # Spinner variants
├── pages/          # (Empty — planned page components)
├── services/       # (Empty — planned API service layer)
├── api.rs          # Axum router definition (SSR only)
├── config.rs       # TOML configuration (SSR only)
├── error.rs        # AdminError enum (SSR only)
├── handlers.rs     # HTTP handlers (SSR only)
├── state.rs        # Application state (SSR only)
└── ui.rs           # Maud HTML templates (SSR only)

styles/
├── variables.css   # shadcn-style CSS custom properties (light + dark)
└── main.css        # Full CSS framework (reset, grid, flex, spacing, animations)

index.html          # Trunk entry point
Trunk.toml          # Trunk build configuration
```

---

## Client-Side Rendering (Default Mode)

### Application Startup

`run_app()` in `lib.rs`:
1. Initializes `wasm_logger` for browser console logging.
2. Creates `yew::Renderer::<App>::new().render()` — mounts the Yew app to the DOM.

### Routing

`Route` enum with 6 routes:

| Route | Path | Component |
|-------|------|-----------|
| `Dashboard` | `/` | `pages::Dashboard` |
| `Repositories` | `/repositories` | `pages::Repositories` |
| `RepositoryDetail` | `/repositories/:id` | `pages::RepositoryDetail` |
| `Statistics` | `/stats` | `pages::Statistics` |
| `Settings` | `/settings` | `pages::Settings` |
| `NotFound` | `/404` | `pages::NotFound` |

**Note:** `pages/` module is currently empty — page components are declared in `lib.rs`'s `switch()` but not yet implemented.

### Components (8 Yew Function Components)

| Component | Props | Description |
|-----------|-------|-------------|
| `Navbar` | none | Sticky top nav with brand (`🌳 Worktree Admin`), route links with emoji icons, theme toggle (🌙) and refresh (🔄) buttons. All styled with inline CSS using shadcn CSS variables. |
| `Footer` | none | Copyright year, links (GitHub, Docs, API, Support), version display. |
| `Card` / `CardFooter` | `title`, `class`, `children`, `header_actions` | Generic container with optional title header and body. |
| `Badge` | `children`, `variant`, `class`, `outlined`, `pill` | Status indicator with 6 color variants: Success, Warning, Error, Secondary, Primary, Info. |
| `Button` | `variant`, `onclick`, `disabled`, `button_type`, `class`, `children` | Styled button with Primary/Secondary/Danger variants. |
| `StatCard` | `icon`, `value`, `label`, `trend`, `class` | Dashboard stat display with icon + value + label + optional trend indicator. |
| `RepoCard` | `repo: RepositoryInfo`, `on_click` | Full repository card: name + status badge, path (📁), stats (🌿 branches, 📝 commits, 💾 size), last activity (🕒). |
| `Loading` / `SimpleLoading` / `FullscreenLoading` | `message`, `size`, `fullscreen` | Spinner with small/medium/large sizes and optional fullscreen overlay. |

---

## Shared Data Types (`types.rs`)

| Type | Key Fields |
|------|-----------|
| `ServerStatus` | `id`, `name`, `running`, `uptime_seconds`, `active_connections`, `tracked_repositories`, `last_updated` |
| `RepositoryInfo` | `id`, `name`, `path`, `branch_count`, `commit_count`, `last_activity`, `size_bytes`, `status` |
| `RepositoryStatus` | `Active`, `Idle`, `Syncing`, `Error` |
| `ServerStats` | `total_repositories`, `total_commits`, `total_branches`, `total_storage_bytes`, `total_operations` |
| `AppSettings` | `theme: Theme`, `auto_refresh`, `refresh_interval_secs`, `items_per_page` |
| `Theme` | `Light`, `Dark`, `Auto` |
| `ApiResponse<T>` | Tagged union: `Success { data }` or `Error { message }` |

All types implement `Serialize`/`Deserialize` for API transport and `Clone`/`PartialEq` for Yew component rendering.

---

## Styling System (`utils.rs` + `styles/`)

The app uses a **shadcn/ui-inspired CSS variable system**:

### CSS Variables (`styles/variables.css`)
Defines HSL color tokens for light (`:root`) and dark (`[data-theme="dark"]`) themes:
- Background, foreground, card, popover, primary, secondary, muted, accent, destructive, success, warning, info, border, input, ring, radius
- 5 chart colors

### CSS Framework (`styles/main.css`)
Hand-written utility classes (Tailwind-like):
- CSS Reset, Typography (h1-h6), Layout (.app, .main-content, .container)
- Grid system (.grid-cols-1 through .grid-cols-4)
- Flexbox, spacing, width/height, text, opacity, cursor, position utilities
- Animations: spin, fadeIn, skeleton shimmer

### Rust Inline Style Helpers (`utils.rs`)
Sub-modules generate CSS strings referencing `hsl(var(--*))` variables:
- `colors` — `bg()`, `text()`, `border()` + presets
- `layout` — `flex()`, `flex_col()`, `items_center()`, `gap()`, `padding()`, `margin()`
- `border` — `rounded()`, `rounded_lg()`, `border_primary()`
- `shadow` — `sm()`, `md()`, `lg()`
- `transition` — `all()`, `colors()`
- `typography` — `text_xs()` through `text_3xl()`, font weights

### Build Pipeline (Trunk)
- Pre-build + build hooks run Tailwind CSS: `npx tailwindcss -i styles/input.css -o dist/output.css --minify`
- Release profile: `opt-level = "z"`, LTO, single codegen unit (optimized for WASM size)

---

## Server-Side Rendering (`--features ssr`)

### Configuration (`config.rs`)

`AdminConfig` with 4 sections:
- `ServerConfig` — host (127.0.0.1), port (3000), worker threads, max connections (1000), request timeout (30s)
- `UiConfig` — title, theme, page size (20), realtime updates, refresh interval (30s)
- `SecurityConfig` — auth enabled, API key, CORS, TLS cert/key paths
- `LoggingConfig` — level, format, file logging, max size (100MB)

Methods: `from_file()`, `from_str()`, `to_file()`, `validate()`, `bind_address()`

Validation: port > 0; if auth enabled, API key must be set.

### State Management (`state.rs`)

`AdminState` is `Clone`-able shared state:
- `config: Arc<AdminConfig>`
- `server_connection: Arc<RwLock<ServerConnection>>` — tracks connection status, server ID, attempt counts, uptime
- `metrics: Arc<RwLock<Metrics>>` — request/error counters, started_at, error rate

### Error Handling (`error.rs`)

`AdminError` enum with 11 variants mapping to HTTP status codes:
- `ServerConnection` → 502 Bad Gateway
- `Authentication` → 401 Unauthorized
- `Authorization` → 403 Forbidden
- `NotFound` → 404 Not Found
- `InvalidRequest` → 400 Bad Request
- `Internal` → 500 Internal Server Error

Implements `IntoResponse` for Axum — returns JSON error bodies with tracing.

### API Router (`api.rs`)

Routes nested under `/api`:

| Method | Path | Handler |
|--------|------|---------|
| GET | `/health` | `health` |
| GET | `/status` | `server_status` |
| GET | `/metrics` | `metrics` |
| POST | `/server/start` | `start_server` |
| POST | `/server/stop` | `stop_server` |
| POST | `/server/restart` | `restart_server` |
| GET | `/repositories` | `list_repositories` |
| GET | `/repositories/:id` | `get_repository` |
| GET | `/stats` | `get_stats` |
| POST | `/maintenance/gc` | `run_garbage_collection` |

All routes go through `auth_middleware` (Bearer token validation, skippable if auth disabled). Router includes CORS (permissive) and tracing layers.

### HTTP Handlers (`handlers.rs`)

- `auth_middleware` — Extracts `Authorization: Bearer <key>`, validates against config API key.
- `health` — Returns `{ status: "healthy", timestamp }`.
- `server_status` — Returns mock `ServerStatus`.
- `list_repositories` — Returns 2 mock `RepositoryInfo` entries.
- `get_repository` — Returns mock repository by ID.
- `get_stats` — Returns mock `ServerStats`.
- Server control (start/stop/restart) — Toggles `ServerConnection` state.
- Garbage collection — Returns mock reclaimed bytes.

### HTML Templates (`ui.rs`)

Maud-based server-side rendering:
- `layout(title, content)` — Full HTML document.
- `dashboard_page(status, stats, repos)` — 4 status cards + server detail + recent repos + chart placeholder.
- `repositories_page(repos)` — Search/filter + grid of repo cards.
- `stats_page(stats)` — 5 stat cards + detailed breakdown placeholder.
- `settings_page()` — General settings form + security form.
- `error_page(code, message)` — Error display.

---

## TODO

- [ ] Implement page components in `pages/` module (Dashboard, Repositories, Statistics, Settings, NotFound)
- [ ] Implement API service layer in `services/` module (HTTP client calls to worktree-server)
- [ ] Replace mock data in handlers with real worktree-server integration
- [ ] Add WebSocket support for real-time dashboard updates
- [ ] Implement repository detail page with branch/snapshot visualization
- [ ] Add dark mode toggle functionality (currently UI-only)
- [ ] Implement settings persistence (save to localStorage / server config)
- [ ] Add authentication flow (login page, token management)
- [ ] Implement repository search and filtering
- [ ] Add pagination for repository and snapshot lists
- [ ] Implement server-side log streaming
- [ ] Add performance monitoring charts (chart.js or similar via WASM)
- [ ] Implement multi-server management (connect to multiple worktree-server instances)
- [ ] Add keyboard shortcuts for common navigation
- [ ] Implement responsive mobile layout
- [ ] Add integration tests for Axum API handlers
- [ ] Implement proper error boundary components for Yew
- [ ] Add accessibility (ARIA labels, keyboard navigation) to all components