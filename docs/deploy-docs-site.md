# Deploying the website and docs to Railway

The public website and documentation (`apps/web`, Next.js + Fumadocs) deploy to
[Railway](https://railway.com) as one Docker service. The W0rkTree server itself
is deployed separately; see [Deployment](./deploy.md).

## How it fits together

| Piece | Role |
|---|---|
| [`apps/web/Dockerfile`](../apps/web/Dockerfile) | Multi-stage build from the repository root (npm workspaces): installs the `apps/web` workspace, runs `npm run build`, ships the Next.js standalone server on `node:24-alpine` as a non-root user. Listens on `PORT` (default 8080). |
| [`apps/web/railway.json`](../apps/web/railway.json) | Railway config-as-code: Dockerfile builder, watch patterns (`apps/web/**`, root `package.json`, `package-lock.json`, `.npmrc`), health check on `/`, restart on failure. |
| [`.railwayignore`](../.railwayignore) | Keeps `railway up` uploads small: only the root manifests and `apps/` are needed by the build. |
| [`scripts/deploy-docs.sh`](../scripts/deploy-docs.sh) | `setup` (service, variables, domain), `check` (lint, link check, build), `deploy` (check, then `railway up --ci`). |
| [`.github/workflows/deploy-docs.yml`](../.github/workflows/deploy-docs.yml) | On every push to `main` that touches the site: lint + link check + build, then deploy with the Railway CLI. Skipped when `RAILWAY_TOKEN` is not set. |

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
   - **Config-as-code > Railway Config File**: `/apps/web/railway.json`
   - **Source > Root Directory**: leave empty (repository root).
5. Optional custom domain: `railway domain docs.example.com --service docs --port 8080`,
   then add the DNS records it prints.

## Deploying

From a checkout (runs lint, the link checker and a production build first):

```bash
bash scripts/deploy-docs.sh deploy
```

Skip the local checks with `--no-check`. The deploy streams Railway's build
logs and fails when the image build fails; Railway then health-checks `/`
before switching traffic, so a broken build never replaces the running site.

### Continuous deployment from GitHub

The **Deploy docs** workflow deploys `main` automatically:

1. In Railway, create a **project token** for the production environment
   (project > Settings > Tokens).
2. In GitHub (repository > Settings > Secrets and variables > Actions), add the
   secret `RAILWAY_TOKEN`. If the service is not called `docs`, add the
   variable `RAILWAY_DOCS_SERVICE`.
3. Optional: create the GitHub environment `docs-production` to require
   approval before deploys.

The workflow runs on pushes to `main` that change `apps/web/**`, the root npm
manifests, `.railwayignore`, the deploy script or the workflow itself, and can
be started by hand (**Run workflow**). Deploys never overlap (one concurrency
group).

**Choose one trigger.** Railway can also deploy on its own when the service is
connected to the GitHub repository (Source > Connect repo); it then uses the
watch patterns in `railway.json`. Use either that or the workflow, not both,
or every push deploys twice. The workflow has the advantage of running the link
checker before deploying.

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
- The image is stateless: no volumes or databases are needed, and it can run
  more than one replica.
- Local check of the production image (Docker required):

  ```bash
  docker build -f apps/web/Dockerfile -t worktree-web .
  docker run --rm -p 8080:8080 worktree-web
  ```
