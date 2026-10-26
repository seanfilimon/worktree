# Deploying the website and docs to Railway

The public website and documentation (`apps/web`, Next.js + Fumadocs) deploy to
[Railway](https://railway.com) as one Docker service. **Railway's GitHub
integration is the deployer**: the service is connected to this repository and
deploys `main` itself. GitHub Actions only checks the site and never deploys.
The W0rkTree server is deployed separately; see [Deployment](./deploy.md).

## How it fits together

| Piece | Role |
|---|---|
| [`apps/web/Dockerfile`](../apps/web/Dockerfile) | Multi-stage build from the repository root (npm workspaces): installs the `apps/web` workspace, runs `npm run build`, ships the Next.js standalone server on `node:24-alpine` as a non-root user. Listens on `PORT` (default 8080). |
| [`apps/web/railway.json`](../apps/web/railway.json) | Railway config-as-code: Dockerfile builder, watch patterns (`apps/web/**`, root `package.json`, `package-lock.json`, `.npmrc`; other commits do not redeploy), health check on `/`, restart on failure. |
| Railway GitHub integration | Deploys every push to `main` that matches the watch patterns. With **Wait for CI**, it first waits for the GitHub checks on the commit. |
| [`.github/workflows/docs-site-checks.yml`](../.github/workflows/docs-site-checks.yml) | **Docs site checks (does not deploy).** Lint, link check and production build on pushes to `main` and pull requests that touch the site. The check Railway waits for. |
| [`scripts/deploy-docs.sh`](../scripts/deploy-docs.sh) | `setup` (service, variables, domain), `check` (the same checks locally), `deploy` (manual `railway up --ci`, for emergencies only). |
| [`.railwayignore`](../.railwayignore) | Keeps manual `railway up` uploads small. Not used by the GitHub integration. |

The service is built from the **repository root** (the Dockerfile needs the
root `package-lock.json`); only the config file lives under `apps/web`.

## One-time setup

1. Install the Railway CLI (`npm i -g @railway/cli`, or let the script use
   `npx @railway/cli@5.66.0`) and sign in: `railway login`.
2. Create or link a project from the repository root: `railway init` (new
   project) or `railway link` (existing one).
3. Create the service, its variables and a public domain:

   ```bash
   bash scripts/deploy-docs.sh setup
   ```

   This creates the service `docs` (override with `RAILWAY_DOCS_SERVICE`), sets
   `PORT=8080`, `RAILWAY_DOCKERFILE_PATH=apps/web/Dockerfile` and
   `NEXT_TELEMETRY_DISABLED=1`, and generates a `*.up.railway.app` domain
   routed to port 8080.
4. In the Railway dashboard, open the service > **Settings**:
   - **Source**: connect the GitHub repository `seanfilimon/worktree`, branch
     `main`, and leave **Root Directory** empty (repository root).
   - **Config-as-code > Railway Config File**: `/apps/web/railway.json`
   - **Wait for CI**: on (see the caveat below).
5. Optional custom domain: `railway domain docs.example.com --service docs --port 8080`,
   then add the DNS records it prints.

No Railway token is needed in GitHub.

## Deploying

Merge or push to `main`. For a commit that changes the site:

1. GitHub runs **Docs site checks (does not deploy)**: lint, link check, build.
2. Railway sees the push through its GitHub integration. With Wait for CI on,
   it waits for the commit's checks and deploys only if they pass; otherwise it
   marks the deployment skipped.
3. Railway builds `apps/web/Dockerfile`, health-checks `/`, then switches
   traffic, so a broken build never replaces the running site.

Railway reports each deployment back to GitHub as the `legionedge / production`
environment (repository > Deployments), with a link to the build logs.

**Wait for CI caveat.** Railway waits for *every* check on the commit, not only
the docs checks. While the main **CI** workflow fails on `main`, docs deploys
with Wait for CI on are skipped. Keep Wait for CI off until CI is green, or fix
CI first; the docs checks still run on every change either way.

**Manual deploy** (emergencies, e.g. GitHub is down) from a checkout:

```bash
bash scripts/deploy-docs.sh deploy
```

It runs the same checks locally, then `railway up --ci` (needs `railway login`
and `railway link`, or a project token in `RAILWAY_TOKEN`).

## Troubleshooting "Application failed to respond"

Railway's proxy reached the service but nothing answered on the domain's port.

1. **Port.** Open the service's deploy logs and find Next's `- Network: http://[::]:PORT`
   line. In Settings > Networking, the public domain's target port must be that
   port (8080 after `setup`, which sets `PORT=8080`). If you created the domain
   in the dashboard with another port, edit it or delete it and run
   `railway domain --service docs --port 8080`.
2. **Bind address.** The image binds to `::` (all IPv6 and IPv4 addresses)
   regardless of the `HOSTNAME` Railway injects; images built before this fix
   listened on the container name only. Redeploy to pick it up. `WEB_BIND_HOST`
   overrides the address if you ever need to.
3. **Config file.** If the dashboard does not show the health check on `/`,
   the service is not reading `/apps/web/railway.json` (Settings >
   Config-as-code), so a broken build can go live. Set it and redeploy.
4. **Crash on start.** `railway logs --service docs` shows the runtime log;
   a missing `- Network:` line means the server never started.

## Operating

- Logs: `railway logs --service docs`; deployments: `railway deployment list --service docs`.
- Roll back: redeploy an earlier deployment from the dashboard, or `git revert`
  and push.
- A commit did not deploy? Check that it touched a watch pattern
  (`apps/web/**`, root npm manifests), then the commit's checks (Wait for CI
  skips deploys when any check fails), then the service's Deployments tab.
- The image is stateless: no volumes or databases are needed, and it can run
  more than one replica.
- Local check of the production image (Docker required):

  ```bash
  docker build -f apps/web/Dockerfile -t worktree-web .
  docker run --rm -p 8080:8080 worktree-web
  ```
