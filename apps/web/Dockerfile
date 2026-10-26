# syntax=docker/dockerfile:1.7
#
# W0rkTree marketing + docs site (Next.js standalone output).
# Build from the repository root (npm workspaces):
#
#   docker build -f apps/web/Dockerfile -t worktree-web .
#
# Railway builds this file through the root railway.json; see
# docs/deploy-docs-site.md.

ARG NODE_VERSION=24

FROM node:${NODE_VERSION}-alpine AS base
RUN apk add --no-cache libc6-compat
ENV NEXT_TELEMETRY_DISABLED=1

# npm workspace manifests only: keeps the `npm ci` layer cached until a
# package.json or the lockfile changes.
FROM base AS manifests
WORKDIR /src
COPY apps ./apps
RUN find apps -mindepth 2 -maxdepth 2 ! -name package.json -exec rm -rf {} +

# Install dependencies (hoisted root node_modules + any nested workspace ones).
# No BuildKit cache mount: Railway rejects cache mounts whose id is not
# prefixed with its service id, and the layer cache covers this step.
FROM base AS deps
WORKDIR /app
COPY package.json package-lock.json .npmrc ./
COPY --from=manifests /src/apps ./apps
RUN npm ci --no-audit --no-fund --workspace=apps/web --include-workspace-root \
    && npm cache clean --force

# Build the site.
FROM deps AS builder
WORKDIR /app
COPY apps/web ./apps/web
ENV NODE_ENV=production
RUN npm run build --workspace=apps/web

# Production image. In a workspace, Next traces from the repo root, so the
# standalone server lives at apps/web/server.js inside .next/standalone.
# PORT defaults to 8080; platforms such as Railway may override it.
FROM base AS runner
WORKDIR /app
ENV NODE_ENV=production \
    PORT=8080

RUN addgroup -S -g 1001 nodejs && adduser -S -D -H -u 1001 -G nodejs nextjs

COPY --from=builder --chown=nextjs:nodejs /app/apps/web/.next/standalone ./
COPY --from=builder --chown=nextjs:nodejs /app/apps/web/.next/static ./apps/web/.next/static
COPY --from=builder --chown=nextjs:nodejs /app/apps/web/public ./apps/web/public

USER nextjs
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
  CMD wget -q -O /dev/null "http://127.0.0.1:${PORT:-8080}/" || exit 1

# server.js binds to $HOSTNAME. Platforms such as Railway set HOSTNAME to the
# container name at runtime, overriding an image ENV, and Next then listens on
# one internal address only ("Application failed to respond"). Bind
# explicitly instead: "::" accepts IPv6 and IPv4 (WEB_BIND_HOST overrides).
CMD ["sh", "-c", "HOSTNAME=\"${WEB_BIND_HOST:-::}\" exec node apps/web/server.js"]
