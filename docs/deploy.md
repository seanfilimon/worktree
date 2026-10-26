# Deploying W0rkTree

This guide covers running the W0rkTree server (`worktree-server`: gRPC sync, REST/SSE API and the
admin panel on **one port**, default `7420`) in production, and installing the client binaries
(`wt`, `worktree-bg`) on developer machines.

| Method | Files | Best for |
|---|---|---|
| [Docker](#docker) | `deploy/docker/Dockerfile`, `deploy/docker/Dockerfile.cli` | One host, quick start |
| [Docker Compose](#docker-compose) | `deploy/docker-compose.yml` | One host with MinIO (S3) / Mailpit (SMTP) |
| [Kubernetes (Helm)](#kubernetes-helm) | `deploy/helm/worktree/` | Clusters |
| [systemd](#systemd) | `deploy/systemd/` | Bare-metal / VM Linux |
| [launchd](#launchd-macos) | `deploy/launchd/` | macOS hosts and developer laptops |
| [Install scripts](#installing-the-clients) | `scripts/install.sh`, `scripts/install.ps1` | `wt` + `worktree-bg` on workstations |

Whatever you choose, read [reverse proxies](#reverse-proxies-and-tls), [backups](#backups) and
[upgrades](#upgrades) before going live. The full list of server flags and `WT_*` environment
variables is in [configuration.md](./configuration.md#server-flags-and-environment); every flag has
an environment variable, flags win over the environment.

---

## Server essentials

- **Single process, single data directory.** Metadata is one bbolt file (`<data>/meta.db`, opened
  with an exclusive lock) and objects live in `<data>/objects/` (or S3 from v0.7). Run exactly one
  server per data directory; scale vertically.
- **Bootstrap the admin** with `WT_ADMIN_PASSWORD` (and optionally `WT_ADMIN_USER`, default
  `admin`) on the first start. The account persists; the variable can be removed afterwards.
- **Health**: `GET /healthz` returns `{"status":"ok","version":…,"uptime_seconds":…}` without
  authentication; `GET /metrics` is Prometheus text. Both bypass rate limits.
- **Admin panel**: the static build of `apps/admin` served from `--admin-dir` at `/admin/`. The
  container image ships it in `/app/admin`; release archives ship it as
  `worktree-admin-<version>.tar.gz`.
- **Registration**: `WT_ALLOW_REGISTRATION=false` for private servers (accounts are then created
  by admins or SSO).

## Docker

The server image is built from the repository root:

```bash
docker build -f deploy/docker/Dockerfile --build-arg VERSION=0.7.0 -t worktree-server .
docker run -d --name worktree -p 7420:7420 \
  -v wt-data:/data \
  -e WT_ADMIN_PASSWORD='change-me' \
  -e WT_ALLOW_REGISTRATION=false \
  worktree-server
curl http://localhost:7420/healthz
```

Release images are published to `ghcr.io/seanfilimon/worktree-server:<version>` (and `:<major>.<minor>`,
`:latest`) for `linux/amd64` and `linux/arm64`.

| Image property | Value |
|---|---|
| Base | Alpine, with `git` (push mirrors), `curl` (health check), `tini` (PID 1) |
| User | `10001:10001`, non-root; `HOME=/data` |
| Volume | `/data` (`WT_DATA`) |
| Admin panel | `/app/admin` (`WT_ADMIN_DIR`) |
| Port | `7420` (`WT_ADDR=:7420`) |
| Health check | `wt-healthcheck`: `GET /healthz` on the `WT_ADDR` port, HTTPS when `WT_TLS_CERT` is set; override with `WT_HEALTHCHECK_URL` |
| Entrypoint | `tini -- worktree-server`; extra arguments are passed as flags |

Configuration is environment-first, so `docker run … -e WT_LOG_LEVEL=debug` and
`docker run … worktree-server --log-level debug` are equivalent. Mount TLS material read-only and
point `WT_TLS_CERT` / `WT_TLS_KEY` at it to serve TLS from the container.

### CLI image

`deploy/docker/Dockerfile.cli` builds statically linked (musl) `wt` and `worktree-bg` and verifies
that they run in an empty `scratch` stage. Use it in CI jobs or to extract static Linux binaries:

```bash
docker build -f deploy/docker/Dockerfile.cli -t worktree-cli .
docker run --rm -v "$PWD:/work" -e WT_TOKEN worktree-cli status
docker create --name wtcli worktree-cli && docker cp wtcli:/usr/local/bin/wt ./wt && docker rm wtcli
```

Published as `ghcr.io/seanfilimon/worktree-cli:<version>`.

## Docker Compose

`deploy/docker-compose.yml` runs the server and, behind
[profiles](https://docs.docker.com/compose/how-tos/profiles/), the companions used by the v0.7
storage and notification features:

```bash
cd deploy
# server only (objects on the wt-data volume)
WT_ADMIN_PASSWORD=change-me docker compose up -d --build

# + MinIO as the S3 object store (console: http://localhost:9001)
WT_ADMIN_PASSWORD=change-me WT_OBJECT_STORE=s3 docker compose --profile s3 up -d

# + Mailpit capturing notification e-mail (UI: http://localhost:8025)
WT_ADMIN_PASSWORD=change-me WT_SMTP_URL=smtp://mailpit:1025 docker compose --profile smtp up -d
```

| Profile | Services | Server settings it pairs with |
|---|---|---|
| (none) | `server` | — |
| `s3` | `minio`, `minio-init` (creates the bucket, then exits) | `WT_OBJECT_STORE=s3`; `WT_S3_*` default to the MinIO service |
| `smtp` | `mailpit` | `WT_SMTP_URL=smtp://mailpit:1025` |

All `WT_*` variables in the file can be set in the shell or in `deploy/.env`. The server waits for
`minio-init` / `mailpit` only when their profile is active (`depends_on.required: false`, Compose
2.20+). Data lives in the named volumes `wt-data` and `minio-data`; `docker compose down` keeps
them, `down -v` deletes them.

## Kubernetes (Helm)

```bash
helm install wt deploy/helm/worktree -n worktree --create-namespace \
  --set server.publicURL=https://wt.example.com \
  --set ingress.enabled=true --set ingress.className=nginx \
  --set 'ingress.hosts[0].host=wt.example.com' \
  --set 'ingress.tls[0].secretName=wt-tls' --set 'ingress.tls[0].hosts[0]=wt.example.com' \
  --set 'server.trustedProxies={10.0.0.0/8}'
kubectl -n worktree get secret wt-worktree -o jsonpath='{.data.WT_ADMIN_PASSWORD}' | base64 -d; echo
helm test wt -n worktree
```

Packaged charts are attached to each GitHub release (`worktree-<version>.tgz`).

What the chart creates:

| Object | Purpose |
|---|---|
| `StatefulSet` (1 replica) | The server, with startup/liveness/readiness probes on `/healthz`, read-only root filesystem, non-root user `10001`, all capabilities dropped, an in-memory `/tmp` |
| `PersistentVolumeClaim` (volume claim template `data`, 20 Gi) | `/data`; or `persistence.existingClaim`, or an `emptyDir` with `persistence.enabled=false` (demo only). Claims are retained on uninstall |
| `ConfigMap` | Every non-secret setting as `WT_*` environment (`server.*`, `objectStore`, `s3.*`, `smtp.from`, `backup.*`) |
| `Secret` | `WT_ADMIN_PASSWORD` (from `adminPassword`, or generated once and kept across upgrades), `WT_S3_ACCESS_KEY` / `WT_S3_SECRET_KEY`, `WT_SMTP_URL`. Skipped when `existingSecret` names your own Secret (keys configurable in `secretKeys`) |
| `Service` + headless `Service` | Port `http` with `appProtocol: kubernetes.io/h2c` (gRPC and HTTP share it) |
| `Ingress` (+ `-grpc` Ingress) | Optional. The second Ingress routes `/worktree.sync.v1.SyncService` with `ingress.grpc.annotations` (default `nginx.ingress.kubernetes.io/backend-protocol: GRPC`) for controllers that choose the backend protocol per Ingress. Set `ingress.grpc.enabled=false` for controllers that honour `appProtocol` (Traefik, Contour, Envoy Gateway) |
| `ServiceAccount` | Without a mounted API token |
| `Pod` (`helm test`) | Calls `/healthz` through the Service |
| `ServiceMonitor` | Optional (`metrics.serviceMonitor.enabled`, needs the Prometheus Operator CRDs): scrapes `/metrics` on port `http` |
| `NetworkPolicy` | Optional (`networkPolicy.enabled`): ingress to the server port from `networkPolicy.ingressFrom` (empty = anyone; this release's CronJob and test pod stay allowed); egress all by default, or DNS + `networkPolicy.egress.extra` with `egress.allowAll=false` |
| `PodDisruptionBudget` | Optional (`podDisruptionBudget.enabled`): exactly one of `minAvailable` / `maxUnavailable` (default `maxUnavailable: 1`). With one replica, `minAvailable: 1` blocks node drains |
| `CronJob` (`<release>-backup`) | Optional example (`backup.cronJob.enabled`): `curl -X POST /api/v1/admin/backups` on `backup.cronJob.schedule` with a server-admin token from `backup.cronJob.tokenSecret` (required); with `backup.target=s3` the archives land in the bucket |

Key values (see `deploy/helm/worktree/values.yaml` and `values.schema.json` for all):

| Value | Default | Notes |
|---|---|---|
| `image.repository` / `image.tag` | `ghcr.io/seanfilimon/worktree-server` / chart `appVersion` | |
| `adminPassword`, `existingSecret` | empty | See the Secret row above |
| `server.publicURL` | empty | Used for SSO redirects and links |
| `server.trustedProxies` | `[]` | **Set to the ingress controller's pod CIDR** so audit, rate limits and IP conditions see real clients |
| `server.allowRegistration` | `true` | |
| `server.tls.enabled`, `server.tls.secretName`, `server.tls.clientCASecretName` | off | End-to-end TLS from the pod (probes switch to HTTPS); optional mTLS CA (`ca.crt`) |
| `objectStore`, `s3.*` | `disk` | `s3` needs `s3.bucket`; server v0.7+ |
| `smtp.url`, `smtp.from` | empty | Server v0.7+ |
| `backup.interval`, `backup.keep` | `"0"`, `7` | Scheduled backups; server v0.7+ |
| `backup.target`, `backup.s3Prefix`, `backup.maxAge` | `disk`, `""`, `"0"` | `s3` keeps backups in the `s3.*` bucket (also with `objectStore: disk`; needs `s3.bucket`); prune scheduled backups by age; server v0.8+ |
| `backup.cronJob.*` | off | See the CronJob row above |
| `retention.policyVersions`, `retention.webhookDeliveries`, `retention.webhookDeliveryKeep` | `""`, `720h`, `200` | Server v0.8+ (see [Retention](#retention)) |
| `smtp.deadAfter` | `24h` | Dead-letter queued email that still fails after this long; server v0.8+ |
| `metrics.serviceMonitor.*`, `networkPolicy.*`, `podDisruptionBudget.*` | off | See the rows above |
| `persistence.size`, `persistence.storageClass` | `20Gi`, cluster default | Volume claim templates are immutable: resize the PVC directly |
| `resources` | 100m / 128Mi requests, 1Gi memory limit | |
| `extraEnv`, `extraArgs` | `[]` | Anything else (`--gc-interval`, `TZ`, …) |

The chart refuses inconsistent values at render time (TLS without a Secret, `objectStore=s3`
or `backup.target=s3` without a bucket, an unknown object store, an existing claim with
persistence disabled, the backup CronJob without `tokenSecret.name`, a PodDisruptionBudget with
both or neither bound).

Validate changes with `helm lint --strict deploy/helm/worktree`, or without Helm with
`cd deploy/helm/check && go run .` (renders the chart for every case in `cases/` and checks the
manifests). `bash deploy/validate.sh` runs every available packaging check.

## systemd

`deploy/systemd/worktree-server.service` is a hardened system unit (dedicated `worktree` user,
`StateDirectory=/var/lib/worktree`, `ProtectSystem=strict`, no capabilities, syscall filter). The
environment comes from `/etc/worktree/server.env` (`deploy/systemd/server.env.example`).

```bash
sudo useradd --system --home-dir /var/lib/worktree --shell /usr/sbin/nologin worktree
sudo install -m 0755 worktree-server /usr/local/bin/
sudo mkdir -p /usr/local/share/worktree/admin
sudo tar -xzf worktree-admin-*.tar.gz --strip-components=1 -C /usr/local/share/worktree/admin
sudo install -D -m 0640 -g worktree deploy/systemd/server.env.example /etc/worktree/server.env
sudoedit /etc/worktree/server.env          # WT_ADMIN_PASSWORD, WT_PUBLIC_URL, WT_TRUSTED_PROXIES, ...
sudo install -m 0644 deploy/systemd/worktree-server.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now worktree-server
journalctl -u worktree-server -f
```

To listen on a privileged port without a proxy, add `CapabilityBoundingSet=CAP_NET_BIND_SERVICE`
and `AmbientCapabilities=CAP_NET_BIND_SERVICE` in a drop-in (`systemctl edit worktree-server`).

`deploy/systemd/worktree-bg@.service` runs the background daemon as a **user** unit, one instance
per `$WT_HOME` (the instance name is the escaped path):

```bash
install -D -m 0644 deploy/systemd/worktree-bg@.service ~/.config/systemd/user/worktree-bg@.service
systemctl --user daemon-reload
systemctl --user enable --now "worktree-bg@$(systemd-escape --path "$HOME/.wt").service"
loginctl enable-linger "$USER"     # optional: keep it running while logged out
```

## launchd (macOS)

- `deploy/launchd/dev.w0rktree.bg.plist` — user agent for `worktree-bg run` (from
  `~/.local/bin`, `$HOME` expanded through `/bin/sh`): copy to `~/Library/LaunchAgents/` and
  `launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/dev.w0rktree.bg.plist`.
- `deploy/launchd/dev.w0rktree.server.plist` — system daemon running `worktree-server` as
  `_worktree` with data in `/usr/local/var/worktree`; the install steps are in the file's header.

`worktree-bg service` prints equivalent single-instance definitions (systemd, launchd, Windows Task
Scheduler) with the absolute path of the running binary.

## Reverse proxies and TLS

The server speaks gRPC (HTTP/2) and HTTP/1.1 on the same port. A proxy in front of it must:

1. forward gRPC over **HTTP/2** to the backend (cleartext h2c, or TLS when the server terminates
   TLS itself) — the sync service path prefix is `/worktree.sync.v1.SyncService/`;
2. not buffer `text/event-stream` responses (`GET /api/v1/events`) and allow long-lived streams;
3. allow large request bodies (pushes stream objects; snapshots can be big);
4. set `X-Forwarded-For`, and the server must list the proxy in
   **`--trusted-proxies` / `WT_TRUSTED_PROXIES`** (IPs or CIDRs). Without it every request appears
   to come from the proxy: per-IP rate limits collapse onto one bucket, IP policy conditions
   cannot match and audit entries show the proxy address. Never trust `0.0.0.0/0`.

Also set `WT_PUBLIC_URL` to the external URL (SSO redirect URLs and links).

**Caddy** (automatic TLS, h2c upstream):

```caddyfile
wt.example.com {
    reverse_proxy h2c://127.0.0.1:7420 {
        flush_interval -1
    }
}
```

**nginx** (gRPC and HTTP need separate upstream directives):

```nginx
server {
    listen 443 ssl;
    http2 on;
    server_name wt.example.com;
    ssl_certificate     /etc/letsencrypt/live/wt.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/wt.example.com/privkey.pem;
    client_max_body_size 0;

    location /worktree.sync.v1.SyncService/ {
        grpc_pass grpc://127.0.0.1:7420;
        grpc_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        grpc_read_timeout 1h;
        grpc_send_timeout 1h;
    }
    location / {
        proxy_pass http://127.0.0.1:7420;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_buffering off;          # SSE
        proxy_read_timeout 1h;
    }
}
```

With the proxy on the same host, run the server on loopback (`WT_ADDR=127.0.0.1:7420`) and set
`WT_TRUSTED_PROXIES=127.0.0.1,::1`.

**Traefik**: point a service at `h2c://worktree:7420` (or use the Helm chart with
`ingress.grpc.enabled=false`; Traefik honours the Service's `appProtocol`).

**TLS directly on the server**: `--tls-cert` / `--tls-key` (`WT_TLS_CERT` / `WT_TLS_KEY`, PEM)
serve HTTPS and gRPC over TLS (ALPN `h2`, `http/1.1`) on the same port; `--tls-client-ca` adds
mutual TLS. The server reloads nothing automatically: restart it after renewing certificates (or
let the proxy handle renewal). Listening without TLS on a non-loopback address logs a warning.

## Backups

From v0.7 the server takes consistent online backups (`POST /api/v1/admin/backups`, the admin
panel's **Backups** page, `wt admin backup create`, or on a schedule with `--backup-interval` /
`WT_BACKUP_INTERVAL` and `--backup-keep`) into `<data>/backups/<id>.wtbak`; restore is offline with
`worktree-server restore --data DIR FILE`. See
[`specs/storage/Backends.md`](../crates/worktree-protocol/specs/storage/Backends.md).

From v0.8 backups can go straight to object storage, off the data volume:

```bash
worktree-server --backup-target s3 --backup-interval 24h --backup-keep 14 --backup-max-age 30d \
  --s3-endpoint s3.amazonaws.com --s3-bucket wt-backups --s3-region eu-west-1   # keys via WT_S3_*
```

`--backup-target s3` (`WT_BACKUP_TARGET`) reuses the `--s3-*` connection settings (it works with
`--object-store disk` too) and stores archives under `--backup-s3-prefix` (default
`<s3-prefix>backups/`). To restore, stop the server and list or restore by id straight from the
bucket, with the same target flags:

```bash
worktree-server restore --list --data /var/lib/worktree --backup-target s3 --s3-bucket wt-backups ...
worktree-server restore --data /var/lib/worktree --backup-target s3 --s3-bucket wt-backups ... bk-20261026T030000Z-a1b2c3
```

With the disk target, `worktree-server restore --data DIR ID` reads `DIR/backups/<id>.wtbak` (a
data directory that only contains `backups/` counts as empty). On Kubernetes the chart's optional
`backup.cronJob` triggers backups on a schedule instead of `--backup-interval`. Without the S3
target, copy the backup files off the host (they live on the same volume as the data).

Without that feature, stop the server and copy the whole data directory (`meta.db` and
`objects/`), or snapshot the volume while the server is stopped. A live copy of `meta.db` is not
consistent. With the S3 object store, back up the bucket with your provider's tools as well.

## Retention

From v0.8 a background pass at startup and every hour prunes old operational data (run it now
with `wt admin retention run`; `wt admin retention` and the admin **Backups** page show the
settings and the last pass):

| Data | Flag (env) | Default |
|---|---|---|
| Policy versions | `--policy-version-retention AGE,COUNT` (`WT_POLICY_VERSION_RETENTION`), e.g. `90d,100`; the latest version of a policy is always kept | keep all |
| Audit events | `--audit-retention AGE` (`WT_AUDIT_RETENTION`) | keep all |
| Webhook deliveries | `--webhook-delivery-retention AGE` / `--webhook-delivery-keep N` per webhook | `720h` / `200` |
| Scheduled backups | `--backup-max-age AGE` / `--backup-keep N` | none / `7` |

Notification email is queued in a durable outbox (v0.8): it survives restarts, is retried with
backoff and dead-lettered after `--email-dead-after` (default `24h`). Inspect and retry it with
`wt admin outbox list` / `wt admin outbox retry ID|--all`, or on the admin **Backups** page.

## Upgrades

1. Read the release notes; take a backup.
2. Replace the binary / bump the image tag (`helm upgrade wt deploy/helm/worktree --reuse-values
   --set image.tag=<version>`, `docker compose pull && docker compose up -d`, or reinstall with
   `scripts/install.sh --with-server --version vX.Y.Z` and `systemctl restart worktree-server`).
3. Start the new version on the same data directory. There is one replica, so expect a short
   outage (graceful shutdown waits up to `WT_SHUTDOWN_TIMEOUT`, default 15 s); the Helm chart's
   startup probe allows five minutes for the first start.
4. Check that `GET /healthz` reports the new version, then upgrade the clients
   (`scripts/install.sh` / `install.ps1`).

Downgrades are not supported once a newer server has written its data; restore the backup instead.

## Installing the clients

`scripts/install.sh` (Linux, macOS, Git Bash) and `scripts/install.ps1` (Windows PowerShell 5.1+,
PowerShell 7 on any OS) download the release archive for the machine's OS and architecture, verify
its SHA-256 against the release's `SHA256SUMS` (falling back to the archive's `.sha256` file for
releases before v0.7) and install `wt` and `worktree-bg` (plus `worktree-server` with
`--with-server` / `-WithServer`). Nothing is installed when verification fails.

```bash
curl -fsSL https://raw.githubusercontent.com/seanfilimon/worktree/main/scripts/install.sh | bash
bash scripts/install.sh --version v0.8.0 --dir /usr/local/bin --with-server
bash scripts/install.sh --base-url https://mirror.example.com/worktree/v0.8.0   # internal mirror
bash scripts/install.sh --base-url ./dist                                       # local build
```

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/seanfilimon/worktree/main/scripts/install.ps1))) -AddToPath
.\scripts\install.ps1 -Version v0.8.0 -WithServer -AddToPath
.\scripts\install.ps1 -BaseUrl \\fileserver\worktree\v0.8.0
```

| Option (sh / ps1) | Env | Default |
|---|---|---|
| `--version` / `-Version` | `WT_VERSION` | `latest` |
| `--dir` / `-InstallDir` | `WT_INSTALL_DIR` | `~/.local/bin`; `%LOCALAPPDATA%\Programs\worktree\bin` |
| `--base-url` / `-BaseUrl` | `WT_BASE_URL` | `https://github.com/<repo>/releases/download/<version>` (`…/latest/download` for latest) |
| `--target` / `-Target` | `WT_TARGET` | detected, e.g. `aarch64-apple-darwin` |
| `--repo` / `-Repo` | `WT_REPO` | `seanfilimon/worktree` |
| `--from-source` / `-FromSource` | | build with cargo (and go) instead |

A base URL can be `https://…`, `file:///…` or a plain directory; it must contain `SHA256SUMS` and
the archives, which is exactly what `scripts/release.sh` writes to `dist/`.

## Building releases

`scripts/release.sh` builds the archives locally (the release workflow runs the same script):

```bash
bash scripts/release.sh                          # host target + admin panel -> dist/
bash scripts/release.sh --all                    # every target with an installed Rust toolchain
bash scripts/release.sh --target aarch64-unknown-linux-gnu --skip-admin
```

Per target (`{x86_64,aarch64}-{unknown-linux-gnu,unknown-linux-musl,apple-darwin,pc-windows-msvc}`;
the musl targets need a musl C toolchain — `<arch>-linux-musl-gcc`, `musl-gcc` or
`cargo-zigbuild` — and are skipped without one) it writes
`worktree-<version>-<target>.tar.gz` (`.zip` on Windows) with `wt`, `worktree-bg`,
`worktree-server`, `LICENSE` and `README.md`; plus `worktree-admin-<version>.tar.gz`, a `.sha256`
per archive and `SHA256SUMS`. The version comes from `[workspace.package]` in `Cargo.toml`. Targets
whose Rust standard library is missing (`rustup target add …`) or whose cross linker fails are
skipped with a warning (`--strict` makes that an error); the Go server always cross-compiles.

Pushing a tag `v<version>` (it must equal the `Cargo.toml` version) runs
`.github/workflows/release.yml`: the six archives on native runners, the admin archive, the Helm
chart, `SHA256SUMS`, a GitHub release, and multi-arch `worktree-server` / `worktree-cli` images on
GHCR.
