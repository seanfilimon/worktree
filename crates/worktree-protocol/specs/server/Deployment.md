# W0rkTree Deployment and Release Specification

## Table of Contents

- [1. Overview](#1-overview)
- [2. Release artifacts](#2-release-artifacts)
- [3. Versioning](#3-versioning)
- [4. Release build (`scripts/release.sh`)](#4-release-build-scriptsreleasesh)
- [5. Installers](#5-installers)
- [6. Container images](#6-container-images)
- [7. Docker Compose](#7-docker-compose)
- [8. Helm chart](#8-helm-chart)
- [9. Service manager units](#9-service-manager-units)
- [10. Proxies, client addresses and TLS](#10-proxies-client-addresses-and-tls)
- [11. Server operations](#11-server-operations)
- [12. Continuous integration](#12-continuous-integration)
- [13. Implementation status](#13-implementation-status)
- [14. Roadmap](#14-roadmap)
- [15. References](#15-references)

---

## 1. Overview

W0rkTree ships three programs and one static web application:

| Component | Source | Runs on |
|---|---|---|
| `wt` (CLI) | `crates/worktree-cli` (Rust) | Developer machines, CI |
| `worktree-bg` (daemon) | `crates/worktree-bg` (Rust) | Developer machines, per user |
| `worktree-server` | `services/server-go` (Go, pure Go, `CGO_ENABLED=0`) | One server host / pod |
| Admin panel | `apps/admin` (Vite build, static files) | Served by `worktree-server --admin-dir` at `/admin/` |

The server is a single process owning a single data directory: `meta.db` (bbolt, exclusive file
lock) plus `objects/` (or an S3 bucket from v0.7). Every packaging format therefore runs exactly
one server replica per data directory and persists the data directory on durable storage. The
server listens on one port for gRPC (HTTP/2, h2c or TLS) and HTTP/1.1, so every proxy or load
balancer in front of it must forward HTTP/2 for the gRPC service path.

All server configuration is available as flags and as `WT_*` environment variables (flag >
environment > default); packaging formats configure the server through the environment only,
leaving flags free for operators.

## 2. Release artifacts

A release `v<version>` consists of:

| Artifact | Contents |
|---|---|
| `worktree-<version>-<target>.tar.gz` (Linux, macOS) / `.zip` (Windows) | Directory `worktree-<version>-<target>/` with `wt`, `worktree-bg`, `worktree-server` (`.exe` on Windows), `LICENSE`, `README.md`. Binaries are mode 0755, other files 0644, owner 0:0 |
| `worktree-admin-<version>.tar.gz` | Directory `worktree-admin-<version>/` with the admin panel build (`index.html`, `assets/`) |
| `worktree-<version>.tgz` | Helm chart package |
| `<artifact>.sha256` | `"<sha256 hex>  <file name>"`, one per artifact |
| `SHA256SUMS` | The same lines for every artifact, sorted by file name |
| `ghcr.io/<owner>/worktree-server:<version>` | Server image, `linux/amd64` + `linux/arm64` |
| `ghcr.io/<owner>/worktree-cli:<version>` | Static CLI image, `linux/amd64` + `linux/arm64` |

Targets (Rust triple → Go platform):

| Target | GOOS/GOARCH | Archive |
|---|---|---|
| `x86_64-unknown-linux-gnu` | `linux/amd64` | tar.gz |
| `aarch64-unknown-linux-gnu` | `linux/arm64` | tar.gz |
| `x86_64-apple-darwin` | `darwin/amd64` | tar.gz |
| `aarch64-apple-darwin` | `darwin/arm64` | tar.gz |
| `x86_64-pc-windows-msvc` | `windows/amd64` | zip |
| `aarch64-pc-windows-msvc` | `windows/arm64` | zip |
| `x86_64-unknown-linux-musl` (v0.8) | `linux/amd64` | tar.gz |
| `aarch64-unknown-linux-musl` (v0.8) | `linux/arm64` | tar.gz |

The musl archives hold statically linked `wt` / `worktree-bg` for Alpine and other non-glibc
systems; their `worktree-server` is the same static `CGO_ENABLED=0` Linux binary.

Image tags: `<version>` always; `<major>.<minor>` and `latest` only for releases without a
pre-release suffix. Per-architecture tags `<version>-amd64` / `<version>-arm64` are the manifest
list's members.

## 3. Versioning

The release version is `[workspace.package] version` in the root `Cargo.toml` (SemVer). The
release workflow refuses a tag that differs from it. The server binary gets the same version via
`-ldflags "-X main.version=<version>"`; `/healthz`, `worktree-server --version`, `wt --version` and
`worktree-bg --version` all report it. The Helm chart is packaged with chart `version` and
`appVersion` set to the release version. A version containing `-` is a pre-release.

## 4. Release build (`scripts/release.sh`)

```
scripts/release.sh [--target TRIPLE]... [--all] [--out DIR] [--version VER]
                   [--skip-admin | --admin-only] [--skip-build] [--no-sums]
                   [--strict] [--target-dir DIR]
```

1. Reads the version (§3); `--version` overrides it for local experiments.
2. Targets: the host triple by default, every supported target with `--all`, or the given ones.
3. Per target: `cargo build --release --locked --target T -p worktree-cli -p worktree-bg`, then
   `go build -trimpath -ldflags "-s -w -X main.version=V"` with `CGO_ENABLED=0` and the target's
   `GOOS`/`GOARCH` into `<target-dir>/go/<goos>-<goarch>/`. A target whose Rust standard library
   is not installed, or whose cargo build fails (typically a missing cross linker), is skipped with
   a warning; `--strict` turns both into errors (the release workflow uses `--strict`).
   musl targets (v0.8) additionally need a musl C toolchain for the crates with C code, tried in
   this order: `<arch>-linux-musl-gcc` (exported as `CC_<target>`,
   `CARGO_TARGET_<TARGET>_LINKER` and, when present, `AR_<target>`), `musl-gcc` for x86_64 on an
   x86_64 Linux host, or `cargo zigbuild` (cargo-zigbuild installed); a musl target that is the
   host's own uses the native compiler. Without one the target is skipped (an error with
   `--strict`).
4. Stages the archive directory under `<target-dir>/release-stage/` and packs it. tar.gz archives
   are written with explicit modes (GNU tar `--mode`) so archives built on Windows still carry
   executable bits; zip archives use `zip`, `7z`, Windows `tar.exe -a` or `Compress-Archive`,
   whichever exists.
5. Admin: `npm ci` when `node_modules/` is missing, `npm run build --workspace=apps/admin`, packed
   as `worktree-admin-<version>.tar.gz`.
6. Writes `<archive>.sha256` for every archive it built and, unless `--no-sums`, regenerates
   `SHA256SUMS` over every `worktree-*.tar.gz` / `worktree-*.zip` in the output directory, so
   several invocations can share one directory.

Output defaults to `dist/` (ignored by git).

## 5. Installers

`scripts/install.sh` (POSIX shells, Git Bash) and `scripts/install.ps1` (Windows PowerShell 5.1,
PowerShell 7 on Windows/Linux/macOS) behave identically:

1. **Target**: detected from the OS and architecture (`uname -s/-m`; `OSArchitecture` in
   PowerShell); Apple-silicon machines running under Rosetta get the `aarch64` build. `--target`
   / `-Target` overrides.
2. **Base URL**: `--base-url` / `-BaseUrl` / `WT_BASE_URL` if given; otherwise
   `https://github.com/<repo>/releases/download/v<version>`, or
   `https://github.com/<repo>/releases/latest/download` for `latest`. Accepted forms: `https://`,
   `http://` (with a warning: the checksum then only detects corruption), `file://` URLs and plain
   directory paths. Downloads use HTTPS-only curl/wget (TLS 1.2+) unless the base is `http://`.
3. **Selection and checksum**: download `SHA256SUMS`; pick the line whose file name is
   `worktree-<version>-<target>.<ext>` (for `latest`: any `worktree-*-<target>.<ext>`). If
   `SHA256SUMS` is missing and a version was given, fall back to `<archive>.sha256` (releases
   before v0.7). No checksum source → error.
4. **Verify**: SHA-256 of the downloaded archive must equal the listed hash (case-insensitive);
   otherwise the installer stops **before extracting or installing anything**.
5. **Install**: extract into a temporary directory, check that every requested binary is present
   (`wt`, `worktree-bg`, plus `worktree-server` with `--with-server`), then copy them into the
   install directory (`~/.local/bin`, or `%LOCALAPPDATA%\Programs\worktree\bin` on Windows) with
   mode 0755.
6. **PATH**: print how to add the directory to `PATH` when it is missing (`-AddToPath` updates
   the Windows user `PATH`).

`--from-source` / `-FromSource` builds with `cargo install --locked` (and `go build` for the
server) from the current checkout or a shallow clone instead of downloading.

## 6. Container images

**Server** (`deploy/docker/Dockerfile`, context = repository root):

- Stages: npm workspace manifests → admin build (Node 22) → Go build (`golang:<go.mod version>`,
  `CGO_ENABLED=0`, cross-compiled from `$BUILDPLATFORM` to `$TARGETOS/$TARGETARCH`) → runtime.
- Runtime: Alpine with `ca-certificates`, `git` (push mirrors), `curl`, `tini`, `tzdata`. User and
  group `10001`, `HOME=/data`, volume `/data`, admin panel in `/app/admin`, `EXPOSE 7420`.
- Environment defaults: `WT_ADDR=:7420`, `WT_DATA=/data`, `WT_ADMIN_DIR=/app/admin`; no default
  command arguments, so environment variables are never shadowed by flags.
- Entrypoint `tini -- worktree-server`. Health check `wt-healthcheck`: `GET /healthz` on
  `127.0.0.1:<port of WT_ADDR>`, `https` (unverified) when `WT_TLS_CERT` is set, or
  `WT_HEALTHCHECK_URL`.

**CLI** (`deploy/docker/Dockerfile.cli`):

- Built on `rust:<1>-alpine` (musl, static by default) with static OpenSSL and zlib; libgit2 and
  libssh2 are compiled by their `-sys` crates; `protoc` comes from Alpine (`PROTOC`).
- A `scratch` stage executes `wt --version` and `worktree-bg --version`, so the build fails unless
  both binaries are fully static.
- Runtime: Alpine with CA certificates, user `10001`, `WT_HOME=/home/wt/.wt`, working directory
  `/work`, entrypoint `wt`.

The repository-root `.dockerignore` keeps the context to sources, manifests and lockfiles.

## 7. Docker Compose

`deploy/docker-compose.yml` (project `worktree`):

| Service | Profile | Role |
|---|---|---|
| `server` | — | Built from `deploy/docker/Dockerfile`; port `${WT_PORT:-7420}`; volume `wt-data:/data`; requires `WT_ADMIN_PASSWORD` |
| `minio` | `s3` | S3-compatible store (API 9000, console 9001), volume `minio-data` |
| `minio-init` | `s3` | One-shot `mc mb --ignore-existing` of `${WT_S3_BUCKET:-worktree}` after MinIO is healthy |
| `mailpit` | `smtp` | SMTP sink on 1025, web UI on 8025 |

The server passes every relevant `WT_*` variable through; S3 settings default to the MinIO service
and take effect with `WT_OBJECT_STORE=s3`, SMTP with `WT_SMTP_URL`. Optional dependencies use
`depends_on.<svc>.required: false` so the server starts alone when a profile is inactive.

## 8. Helm chart

`deploy/helm/worktree` (apiVersion v2; Kubernetes 1.27+ recommended for the PVC retention policy):

- **Workload**: `StatefulSet` with `replicas: 1` (not configurable — bbolt lock), governing
  headless Service, `OrderedReady`, rolling updates. Pod: non-root `10001`, `fsGroup 10001`,
  seccomp `RuntimeDefault`, read-only root filesystem, all capabilities dropped, no service-account
  token, in-memory `/tmp`. Config and Secret checksums are pod annotations, so changes roll the pod.
- **Storage**: volume claim template `data` (retained on delete/scale), or `existingClaim`, or
  `emptyDir` when persistence is disabled.
- **Configuration**: a ConfigMap of `WT_*` variables (`envFrom`). `WT_ADDR` follows
  `server.port`; `WT_DATA=/data`; `WT_ADMIN_DIR=/app/admin`. S3, SMTP-sender and backup variables
  are emitted only when those features are configured, so older servers never see them.
- **Secrets**: `WT_ADMIN_PASSWORD`, `WT_S3_ACCESS_KEY`, `WT_S3_SECRET_KEY`, `WT_SMTP_URL` come from
  the chart's Secret or from `existingSecret` (key names in `secretKeys`). Without an explicit
  password a 24-character random one is generated on install and preserved on upgrade via
  `lookup`.
- **Probes**: `startupProbe` (5 s × 60), `livenessProbe`, `readinessProbe` — all `GET /healthz` on
  port `http`, scheme `HTTPS` when `server.tls.enabled`.
- **Server TLS**: `server.tls.secretName` (`kubernetes.io/tls`) mounted at `/etc/worktree/tls`,
  optional `clientCASecretName` (`ca.crt`) at `/etc/worktree/client-ca` for mTLS.
- **Networking**: `Service` port `http` with `appProtocol: kubernetes.io/h2c` (`https` with server
  TLS). Optional `Ingress` for `/`; with `ingress.grpc.enabled` a second Ingress for
  `/worktree.sync.v1.SyncService` carrying `ingress.grpc.annotations` (merged over the shared
  annotations, default nginx `backend-protocol: GRPC`).
- **Server operations** (v0.8): `backup.target` / `backup.s3Prefix` / `backup.maxAge`,
  `retention.policyVersions` / `retention.webhookDeliveries` / `retention.webhookDeliveryKeep` and
  `smtp.deadAfter` map to the §11 flags (`WT_BACKUP_TARGET`, `WT_BACKUP_S3_PREFIX`,
  `WT_BACKUP_MAX_AGE`, `WT_POLICY_VERSION_RETENTION`, `WT_WEBHOOK_DELIVERY_RETENTION`,
  `WT_WEBHOOK_DELIVERY_KEEP`, `WT_EMAIL_DEAD_AFTER`). The `s3` values and credentials are rendered
  when `objectStore=s3` or `backup.target=s3` (`WT_OBJECT_STORE` only for the former).
- **Optional resources** (v0.8, all off by default):
  - `metrics.serviceMonitor.enabled`: a Prometheus Operator `ServiceMonitor` scraping `/metrics`
    on port `http` of the main Service (HTTPS with server TLS, optional
    `tlsInsecureSkipVerify`; `interval`, `scrapeTimeout`, `labels`, `namespace`).
  - `networkPolicy.enabled`: a `NetworkPolicy` for the server pods. Ingress to the server port from
    `ingressFrom` peers (empty = anyone; when restricted, this release's own pods — the backup
    CronJob and the test pod — stay allowed). Egress `allowAll: true` by default (S3, SMTP,
    webhooks, SSO, mirrors keep working under a default-deny namespace); `allowAll: false` allows
    DNS, the server pods and `egress.extra` rules only.
  - `podDisruptionBudget.enabled`: a `PodDisruptionBudget` with exactly one of `minAvailable` /
    `maxUnavailable` (default `maxUnavailable: 1`; with one replica `minAvailable: 1` blocks
    voluntary evictions such as node drains).
  - `backup.cronJob.enabled`: an example `CronJob` (`schedule`, default `0 3 * * *`,
    `concurrencyPolicy: Forbid`) whose curl pod calls `POST /api/v1/admin/backups` with a server
    admin token from `backup.cronJob.tokenSecret` (required; passed on stdin, never on the command
    line) and a note; the request is synchronous, so the job's status is the backup's. Combined
    with `backup.target: s3` this exports backups to object storage on a Kubernetes schedule. The
    pod runs non-root with a read-only root filesystem and no service-account token.
- **Validation**: `values.schema.json` (types, enums, ranges, patterns) plus render-time `fail`s
  for inconsistent combinations (§ docs/deploy.md; v0.8: `backup.target=s3` without `s3.bucket`,
  an enabled CronJob without `tokenSecret.name`, a PDB with both or neither bound).
- **Tests**: a `helm test` pod running `curl /healthz` through the Service.

`deploy/helm/check` is a Helm-free renderer (Go `text/template`, the Sprig subset the chart uses,
`missingkey=error`) that renders every values file in `cases/`, validates the schema subset and
the Kubernetes basics, and checks per-case `# expect:` / `# expect-not:` / `# expect-text:` /
`# expect-error:` assertions. CI runs both it and `helm lint --strict` / `helm template`.

## 9. Service manager units

| File | Kind | Key properties |
|---|---|---|
| `deploy/systemd/worktree-server.service` | system unit | User `worktree`, `StateDirectory=worktree` (`/var/lib/worktree`, `WT_DATA`), `EnvironmentFile=-/etc/worktree/server.env`, `Restart=on-failure`, `ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `PrivateDevices`, no capabilities, `SystemCallFilter=@system-service ~@privileged` (EPERM), `MemoryDenyWriteExecute`, `RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX` |
| `deploy/systemd/server.env.example` | environment file | Documented `WT_*` settings; mode 0640 group `worktree` |
| `deploy/systemd/worktree-bg@.service` | user template | Instance = systemd-escaped `$WT_HOME` path; `WT_HOME=%f`; `ExecStart=%h/.local/bin/worktree-bg run`; `Restart=on-failure` |
| `deploy/launchd/dev.w0rktree.bg.plist` | user agent | Label `dev.w0rktree.bg` (same as `worktree-bg service`), `RunAtLoad`, `KeepAlive.SuccessfulExit=false`, started via `/bin/sh -c` so `$HOME` expands |
| `deploy/launchd/dev.w0rktree.server.plist` | system daemon | Label `dev.w0rktree.server`, user `_worktree`, data `/usr/local/var/worktree`, log `/usr/local/var/log/worktree-server.log`, 65536 open files |

## 10. Proxies, client addresses and TLS

- The client address used for audit, rate limits and IP policy conditions is the socket peer,
  unless the peer is listed in `--trusted-proxies` / `WT_TRUSTED_PROXIES`, in which case the
  right-most untrusted `X-Forwarded-For` entry is used. Every packaging format exposes this
  setting (`server.trustedProxies` in Helm, `WT_TRUSTED_PROXIES` elsewhere).
- Proxies must forward gRPC over HTTP/2, must not buffer `text/event-stream`, and should allow
  unlimited request bodies and hour-long streams. `docs/deploy.md` has Caddy, nginx and Traefik
  configurations.
- TLS terminates either at the proxy/ingress (default) or at the server (`--tls-cert`,
  `--tls-key`, optional `--tls-client-ca`).

## 11. Server operations

v0.8 (`docs/ENGINEERING_PLAN.md` §16.11). Every setting is a flag and a `WT_*` variable; ages
accept Go durations (`720h`) or whole days (`30d`).

**Backups to object storage.** `--backup-target disk|s3` (`WT_BACKUP_TARGET`, default `disk`)
keeps archives in `<data>/backups` or in the `--s3-*` bucket under `--backup-s3-prefix`
(`WT_BACKUP_S3_PREFIX`, default `<s3-prefix>backups/`), independently of `--object-store`.
Scheduled backups are pruned by count (`--backup-keep`) and age (`--backup-max-age`,
`WT_BACKUP_MAX_AGE`). `worktree-server restore --list` lists either target and
`worktree-server restore --data DIR ID` restores a backup id from it (streamed from S3). See
[Backends](../storage/Backends.md).

**Durable email outbox.** Notification email is written to the `outbox` document collection
before it is sent and survives restarts; failed attempts are retried with backoff (30 s, 2 m,
10 m, 30 m, then every 30 m) and dead-lettered after `--email-dead-after` (`WT_EMAIL_DEAD_AFTER`,
default 24 h). Server admins list, retry and delete queued email (`GET /api/v1/admin/outbox`,
`POST .../outbox/{id}/retry`, `POST .../outbox/retry` for every dead letter,
`DELETE .../outbox/{id}`; `wt admin outbox list|retry|delete`; the admin Backups page). See
[Notifications](./Notifications.md) §7.

**Retention.** A background pass runs at startup and hourly (and on demand:
`POST /api/v1/admin/retention/run`, `wt admin retention run`; settings and the last pass:
`GET /api/v1/admin/retention`, `wt admin retention`, admin Backups page):

| Data | Flag (env) | Default | Rule |
|---|---|---|---|
| Policy versions | `--policy-version-retention` (`WT_POLICY_VERSION_RETENTION`) | keep all | `AGE`, `COUNT` or `AGE,COUNT` per policy; the latest version of every policy is always kept |
| Audit events | `--audit-retention` (`WT_AUDIT_RETENTION`) | keep all | Older than the age (also applied by garbage collection) |
| Webhook deliveries | `--webhook-delivery-retention` / `--webhook-delivery-keep` (`WT_WEBHOOK_DELIVERY_RETENTION` / `WT_WEBHOOK_DELIVERY_KEEP`) | `720h` / `200` | Older than the age or beyond the newest N per webhook |
| Scheduled backups | `--backup-max-age` / `--backup-keep` | none / `7` | Older than the age or beyond the newest N; manual backups are kept |

A pass that deleted anything is audited (`retention.run`, actor `system`, counts in the details).

## 12. Continuous integration

`.github/workflows/ci.yml` (push to `main`, pull requests):

| Job | Checks |
|---|---|
| `rust` (Linux, Windows, macOS) | `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace` |
| `go` (Linux, Windows) | `gofmt`, `go vet`, `go build`, `go test` (`-race` on Linux), `WT_INTEGRATION=1` server tests on Linux |
| `go-lint` | golangci-lint (advisory) |
| `proto` | Regenerated Go bindings have no drift |
| `admin` | `npm run lint`, `tsc -b`, `vitest run`, build (Node 22) |
| `web` | `npm run lint` (ESLint + link check), build |
| `e2e` | `scripts/e2e.sh` on Linux |
| `packaging` | `deploy/validate.sh --strict`: shellcheck, PowerShell parse, YAML/JSON structure, `docker compose config` per profile, `helm lint --strict` + `helm template` per case, the Go chart renderer, actionlint, systemd/launchd checks |
| `docker` | Builds both images; server smoke test (`/healthz`, `/admin/`, `git`, health-check script, uid 10001); CLI `--version` |
| `release-smoke` | `scripts/release.sh` for linux/amd64, `install.sh` from the local `dist/` (directory and `file://`), rejection of a corrupted archive, `install.ps1` under pwsh |

`.github/workflows/release.yml` (tags `v*`, or manual with a tag): version check → six target
archives (native runners, `release.sh --strict`) + admin archive + Helm chart → `SHA256SUMS` and
GitHub release (pre-release for `-` versions) ; per-architecture images built natively and merged
into multi-arch manifests.

## 13. Implementation status

| Item | Status |
|---|---|
| Server image (multi-stage, non-root, git, `/data`, health check, `/app/admin`) | Built |
| Static CLI image with static-link proof | Built |
| Compose file with `s3` (MinIO) and `smtp` (Mailpit) profiles | Built |
| Helm chart (StatefulSet, PVC, Service, Ingress + gRPC Ingress, Secret, ConfigMap, probes, TLS, S3, SMTP, backups, schema, tests) | Built |
| Helm-free chart renderer and checks | Built |
| systemd system unit + user template, launchd agent + daemon | Built |
| `scripts/release.sh` (cross targets, admin archive, SHA256SUMS) | Built |
| `install.sh` / `install.ps1` with base URL, `SHA256SUMS`, legacy `.sha256` fallback | Built |
| CI packaging, image and release smoke jobs; release workflow with chart and multi-arch images | Built |
| S3 / SMTP / backup settings consumed by the server | Built (v0.7 `storage-backends`, `notifications`) |
| musl release targets when a musl toolchain exists | Built (v0.8 §16.11) |
| Chart `ServiceMonitor`, `NetworkPolicy`, `PodDisruptionBudget`, backup `CronJob` (off by default), v0.8 server settings, check cases | Built (v0.8 §16.11) |
| Backups to S3, durable email outbox, retention service (§11) | Built (v0.8 §16.11) |

## 14. Roadmap

- Signed release artifacts (Sigstore/cosign signatures for archives and images, SLSA provenance)
  and signature verification in the installers.
- Native packages: Homebrew tap, Scoop/winget manifests, `.deb`/`.rpm`.
- musl targets in the release workflow (needs musl cross toolchains on the CI runners).
- Distroless server image variant (needs a `git`-free mirror implementation).
- Rolling upgrades without downtime (requires a multi-replica metadata store).

## 15. References

- `docs/ENGINEERING_PLAN.md` §15.1, §16.11 — binding contract
- `docs/deploy.md` — operator guide (Docker, Compose, Helm, systemd, proxies, TLS, backups, upgrades)
- `deploy/docker/`, `deploy/docker-compose.yml`, `deploy/helm/`, `deploy/systemd/`, `deploy/launchd/`
- `deploy/validate.sh`, `deploy/validate-files.mjs`, `deploy/helm/check/` — validation
- `scripts/release.sh`, `scripts/install.sh`, `scripts/install.ps1`
- `.github/workflows/ci.yml`, `.github/workflows/release.yml`
- `services/server-go/internal/config/config.go`, `serverops.go` — flags and `WT_*` variables
- `services/server-go/internal/retention/`, `internal/notify/outbox.go`, `internal/backup/target.go`
  — retention service, email outbox, backup targets
- [`Server.md`](./Server.md) — server architecture and operations
