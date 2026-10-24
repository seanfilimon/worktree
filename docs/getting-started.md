# Getting started

This guide takes you from source checkout to a team workflow: build the tools, run a server, create a
worktree, branch and merge, share through the server, and let the daemon publish your work in
progress. Commands are shown for a POSIX shell (Git Bash works on Windows); PowerShell equivalents are
given where they differ.

- [1. Build W0rkTree](#1-build-w0rktree)
- [2. Start a server](#2-start-a-server)
- [3. Create an account](#3-create-an-account)
- [4. Your first worktree](#4-your-first-worktree)
- [Branches and merging](#branches-and-merging)
- [Nested trees](#nested-trees)
- [Sharing through the server](#sharing-through-the-server)
- [Team workflow: staged snapshots and the daemon](#team-workflow-staged-snapshots-and-the-daemon)
- [Protecting branches and merge requests](#protecting-branches-and-merge-requests)
- [Coming from Git](#coming-from-git)
- [Next steps](#next-steps)

---

## 1. Build W0rkTree

Requirements: Rust stable, Go 1.25+, and Node.js 22+ if you want the admin panel. Per-OS setup is in
[CONTRIBUTING.md](../CONTRIBUTING.md#development-setup).

```bash
git clone https://github.com/seanfilimon/worktree.git
cd worktree
cargo build --release                                              # wt, worktree-bg
(cd services/server-go && go build -o ../../target/release/worktree-server .)
(cd apps/admin && npm install && npm run build)                    # optional
export PATH="$PWD/target/release:$PATH"
wt --version
```

```powershell
cargo build --release
Push-Location services\server-go; go build -o ..\..\target\release\worktree-server.exe .; Pop-Location
$env:PATH = "$PWD\target\release;$env:PATH"
wt --version
```

## 2. Start a server

```bash
export WT_ADMIN_PASSWORD='change-me'
worktree-server --data ./wt-data --admin-dir apps/admin/dist
```

The server listens on `http://localhost:7420` for everything: the sync protocol (gRPC), the REST
API, live events, `/healthz`, `/metrics` and the admin panel at `http://localhost:7420/admin/`.
Data goes to `./wt-data` (`meta.db` + `objects/`). The admin account is `admin` with the password
you set. All options: [configuration](./configuration.md#server-flags-and-environment).

```bash
curl -s http://localhost:7420/healthz
# {"status":"ok","version":"0.1.0","uptime_seconds":3}
```

## 3. Create an account

```bash
wt register http://localhost:7420 -u ada -e ada@example.com   # prompts for a password
wt whoami http://localhost:7420
```

Registering creates your personal **tenant** `ada` (you are its owner) and logs you in. The token is
saved in `$WT_HOME/credentials.toml` (default `~/.wt`). On another machine use
`wt login http://localhost:7420 -u ada`. In scripts, pipe the password with `--password-stdin` or set
`WT_PASSWORD`.

## 4. Your first worktree

```bash
wt init demo && cd demo
wt config set user.name "Ada Lovelace"
wt config set user.email ada@example.com

printf 'fn main() {}\n' > main.rs
echo "target/" > .wtignore
wt status                     # lists .wtignore and main.rs as added
wt snapshot -m "Initial snapshot"
echo '// TODO' >> main.rs
wt diff
wt snapshot -m "Add TODO"
wt log --oneline
wt show HEAD --stat
```

There is no `add` step: a snapshot captures the whole tree as it is on disk, minus ignored files.
Before each snapshot, a [secret scan](./security.md#secret-scanning) refuses files that look like they
contain credentials.

Made a mistake?

```bash
wt restore main.rs            # discard uncommitted changes to a file
wt revert HEAD                # add a snapshot that undoes the last one
wt reflog                     # everything that happened, append-only
```

## Branches and merging

```bash
wt switch -c feature/greeting
printf 'fn main() { println!("hi"); }\n' > main.rs
wt snapshot -m "Greet"

wt switch main
wt merge feature/greeting      # fast-forward
wt branch archive feature/greeting
wt branch -a                   # archived branches are still listed with -a
```

When both sides changed the same lines, the merge stops with exit code 3 and writes diff3 markers:

```bash
wt merge feature/other         # reports the conflicted paths, exit code 3
# edit main.rs, resolve and remove the <<<<<<< ||||||| ======= >>>>>>> markers
wt merge --continue            # or: wt merge --abort
```

A machine-readable list of conflicts is in `.wt/conflicts/<tree>.json`.

## Nested trees

A worktree can contain several independently versioned **trees** — think submodules without the
pain. Each has its own branches, history and tags.

```bash
wt tree add web apps/web
echo "<h1>hi</h1>" > apps/web/index.html
wt status                      # root tree: apps/web is excluded
cd apps/web && wt status       # tree "web" (chosen by current directory)
wt snapshot -m "Web skeleton"
cd ../..
wt --tree web log --oneline    # address a tree explicitly from anywhere
wt tree list
```

`wt` picks the deepest tree containing the current directory; `--tree NAME` overrides that.

## Sharing through the server

Point the worktree at a tenant on the server and push:

```bash
wt remote add origin http://localhost:7420 --tenant ada
wt push                        # creates tree ada/demo on first push
wt --tree web push             # each tree is pushed separately (ada/web)
```

A teammate (with a role on tenant `ada`, or with the tree made `internal`/`public`) clones it:

```bash
wt clone http://localhost:7420 ada/demo demo
cd demo
# ...work...
wt snapshot -m "Fix typo"
wt push
```

Back in your copy:

```bash
wt fetch                       # update origin/* only
wt log origin/main --oneline
wt pull                        # fast-forward or merge into the current branch
```

If someone pushed first, your push is rejected with a conflict — history is never overwritten.
Run `wt pull`, then `wt push` again.

To add teammates to an organization, create an org tenant and add members (admin panel, or REST):

```bash
TOKEN=$(echo '{"username":"ada","password":"…"}' | curl -s -X POST -H 'content-type: application/json' \
  -d @- http://localhost:7420/api/v1/auth/login | jq -r .token)
curl -s -X POST -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"slug":"acme","name":"Acme Inc."}' http://localhost:7420/api/v1/tenants
curl -s -X PUT -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"role":"developer"}' http://localhost:7420/api/v1/tenants/acme/members/grace
```

Roles: `viewer` (read), `developer` (push, stage, branches, merge requests), `maintainer` (approve
and merge, create trees), `admin`, `owner`. See the [role matrix](./server-architecture.md#role-matrix).

## Team workflow: staged snapshots and the daemon

Staged snapshots show your teammates what you are working on **before** you push, without touching
branch history.

```bash
wt stage                       # upload the current working state as your staged snapshot
wt status --team               # everyone's staged work on this tree
```

The background daemon does this for you:

```bash
wt daemon start
wt daemon watch                # watch the current worktree
# keep editing; after 30 s of inactivity (or 50 changed files) the daemon
# snapshots ("auto: N files changed") and stages
wt daemon status
wt daemon pause                # stop auto-snapshots for a while
```

Tune it per worktree in `.wt/config.toml` (`[auto_snapshot]`, `[sync]`), see
[configuration](./configuration.md#keys) and [daemon](./daemon.md). Pushing a branch clears your
staged snapshot for it.

## Protecting branches and merge requests

Tree admins can protect branches in the admin panel (tree → **Protections**) or over REST:

```bash
curl -s -X PUT -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"pattern":"main","no_direct_push":true,"required_approvals":1,"allowed_pushers":[],"no_delete":true}' \
  http://localhost:7420/api/v1/trees/acme/platform/protections
```

Now `wt push` to `main` is rejected. Push a branch and open a merge request instead:

```bash
wt push --branch feature/login
curl -s -X POST -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"title":"Login","description":"","source_branch":"feature/login","target_branch":"main"}' \
  http://localhost:7420/api/v1/trees/acme/platform/merge-requests
```

A maintainer approves and merges in the admin panel. Merging fast-forwards `main` to the source
branch; if `main` moved in the meantime, merge `main` into your branch locally, push, and merge again.

## Coming from Git

```bash
wt git import ~/src/legacy legacy       # all branches, tags, .gitignore
cd legacy && wt log --oneline
wt git export ../legacy.git             # and back, incrementally and deterministically
```

| Git | W0rkTree |
|---|---|
| `git init` | `wt init` |
| `git add -A && git commit -m` | `wt snapshot -m` |
| `git checkout -b x` / `git switch -c x` | `wt switch -c x` |
| `git checkout -- file` / `git restore file` | `wt restore file` |
| `git revert` | `wt revert` |
| `git branch -D` | `wt branch archive` (soft delete) |
| `git stash` | `wt stage` (shared WIP) or a branch |
| `git rebase`, `git push --force` | Not available — history is append-only |
| `git submodule` | `wt tree add` |

Details and limits: [Git compatibility](./git-compatibility.md).

## Next steps

- [CLI reference](./cli-reference.md) — every command and flag.
- [Server architecture](./server-architecture.md) — permissions, REST API, events.
- [SDK guide](./sdk-guide.md) — build your own tools on `worktree-sdk`.
