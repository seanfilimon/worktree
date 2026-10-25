# syntax=docker/dockerfile:1.7
#
# W0rkTree server image: the Go server (gRPC h2c + REST/SSE + admin panel on
# one port, :7420) with the React admin panel (apps/admin) as static files in
# /app/admin and `git` for push mirrors.
#
# Build from the repository root:
#
#   docker build -f deploy/docker/Dockerfile -t worktree-server .
#   docker run -p 7420:7420 -v wt-data:/data -e WT_ADMIN_PASSWORD=change-me worktree-server
#
# Configuration is environment-first (WT_* variables, see docs/deploy.md);
# extra flags can be appended to `docker run ... worktree-server --flag`.
# The build stages run on the build host's native platform and cross-compile,
# so multi-arch builds (linux/amd64, linux/arm64) need no emulation.

ARG NODE_VERSION=22
ARG GO_VERSION=1.25
ARG ALPINE_VERSION=3.22

# ---------------------------------------------------------------------------
# npm workspace manifests only: keeps the `npm ci` layer cached until a
# package.json or the lockfile changes.
FROM --platform=$BUILDPLATFORM node:${NODE_VERSION}-alpine AS manifests
WORKDIR /src
COPY apps ./apps
RUN find apps -mindepth 2 -maxdepth 2 ! -name package.json -exec rm -rf {} +

# ---------------------------------------------------------------------------
# Admin panel (Vite) -> /src/apps/admin/dist
FROM --platform=$BUILDPLATFORM node:${NODE_VERSION}-alpine AS admin
WORKDIR /src
ENV CI=1
COPY package.json package-lock.json .npmrc ./
COPY --from=manifests /src/apps ./apps
RUN --mount=type=cache,target=/root/.npm \
    npm ci --no-audit --no-fund --workspace=apps/admin --include-workspace-root
COPY apps/admin ./apps/admin
RUN npm run build --workspace=apps/admin && test -f apps/admin/dist/index.html

# ---------------------------------------------------------------------------
# Go server -> /out/worktree-server (static, pure Go, CGO_ENABLED=0)
FROM --platform=$BUILDPLATFORM golang:${GO_VERSION}-alpine AS server
ARG TARGETOS
ARG TARGETARCH
ARG VERSION=dev
WORKDIR /src
ENV CGO_ENABLED=0 GOFLAGS=-trimpath
COPY services/server-go/go.mod services/server-go/go.sum ./
RUN --mount=type=cache,target=/go/pkg/mod go mod download
COPY services/server-go/ ./
RUN --mount=type=cache,target=/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
    GOOS=$TARGETOS GOARCH=$TARGETARCH go build -trimpath \
      -ldflags "-s -w -X main.version=${VERSION}" \
      -o /out/worktree-server .

# ---------------------------------------------------------------------------
# Runtime: Alpine (git for mirrors, curl for the health check, tini as PID 1
# so git subprocesses are always reaped).
FROM alpine:${ALPINE_VERSION}
ARG VERSION=dev
ARG COMMIT=unknown
LABEL org.opencontainers.image.title="worktree-server" \
      org.opencontainers.image.description="W0rkTree version control server with admin panel" \
      org.opencontainers.image.source="https://github.com/seanfilimon/worktree" \
      org.opencontainers.image.licenses="LicenseRef-W0rkTree-Public-License-1.0" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${COMMIT}"

RUN apk add --no-cache ca-certificates curl git tini tzdata \
 && addgroup -S -g 10001 worktree \
 && adduser -S -D -h /data -u 10001 -G worktree -s /sbin/nologin worktree \
 && mkdir -p /data /app/admin \
 && chown worktree:worktree /data

COPY --from=server /out/worktree-server /usr/local/bin/worktree-server
COPY --from=admin /src/apps/admin/dist /app/admin
COPY --chmod=0755 deploy/docker/healthcheck.sh /usr/local/bin/wt-healthcheck

# Flags take precedence over these, environment variables override them.
ENV WT_ADDR=:7420 \
    WT_DATA=/data \
    WT_ADMIN_DIR=/app/admin \
    HOME=/data

USER 10001:10001
WORKDIR /data
VOLUME ["/data"]
EXPOSE 7420

HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
  CMD ["/usr/local/bin/wt-healthcheck"]

ENTRYPOINT ["/sbin/tini", "--", "/usr/local/bin/worktree-server"]
