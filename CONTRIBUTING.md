# Contributing to W0rkTree

> **W0rkTree is what comes after Git.** Every contribution moves us closer to replacing a tool the
> industry has outgrown.

| Role | Name | Contact |
|---|---|---|
| **Founding Engineer** | **Sean Filimon** | [@seanfilimon](https://github.com/seanfilimon) |

## Table of contents

- [Project philosophy](#project-philosophy)
- [Development setup](#development-setup)
- [Workspace layout](#workspace-layout)
- [Building](#building)
- [Coding standards](#coding-standards)
- [Testing](#testing)
- [Protocol changes and code generation](#protocol-changes-and-code-generation)
- [Documentation](#documentation)
- [Commits and pull requests](#commits-and-pull-requests)
- [Review criteria](#review-criteria)
- [Issue reporting](#issue-reporting)
- [Security vulnerabilities](#security-vulnerabilities)
- [License](#license)
- [Code of conduct](#code-of-conduct)

---

## Project philosophy

Pull requests that violate these principles will be asked to change.

| # | Principle | In practice |
|---|---|---|
| 1 | **One job per command** | Every CLI command does exactly one thing. No flags that change what a command fundamentally does. |
| 2 | **Plain terminology** | Snapshot, not commit. Tree, not repository. If a word needs explaining, it is the wrong word. |
| 3 | **Automatic by default** | Auto-snapshots and auto-staging are on by default; manual control is always available. |
| 4 | **Append-only history** | No rebase, no force-push, no history rewriting — in the client, the server and the protocol. |
| 5 | **Non-destructive operations** | Soft deletes (archive) instead of hard deletes; everything is recorded in the reflog or audit log. |
| 6 | **Real-time collaboration** | Staged snapshot visibility is a protocol feature, not a UI layer. |
| 7 | **Security is not an afterthought** | Authentication, authorization, license checks, secret scanning and auditing are part of the design. |

The binding contract for the current architecture is [docs/ENGINEERING_PLAN.md](./docs/ENGINEERING_PLAN.md).

---

## Development setup

### Toolchain

| Tool | Version | Used for |
|---|---|---|
| Rust | stable (edition 2021) via [rustup](https://rustup.rs), with `rustfmt` and `clippy` | `crates/*` |
| Go | 1.25 or newer | `services/server-go` |
| Node.js | 22 or newer (npm 10+) | `apps/admin`, `apps/web` |
| Git | any recent | Cloning; Git Bash runs the scripts on Windows |
| C toolchain | see below | Building libgit2 (via the `git2` crate) |

You do **not** need to install `protoc`: the Rust build uses a vendored binary
(`protoc-bin-vendored`), and `scripts/gen-proto.sh` reuses it.

### Windows

1. Install the [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
   with the "Desktop development with C++" workload (MSVC linker for Rust).
2. Install rustup (`rustup-init.exe`), then `rustup component add rustfmt clippy`.
3. Install Go from [go.dev/dl](https://go.dev/dl/) and Node.js 22 LTS.
4. Install [Git for Windows](https://git-scm.com/download/win); run `scripts/*.sh` from **Git Bash**.

```powershell
winget install Rustlang.Rustup GoLang.Go OpenJS.NodeJS.LTS Git.Git
rustup component add rustfmt clippy
```

Windows is a first-class platform — the project is developed on it. See
[Windows rules](#windows-rules) below.

### macOS

```bash
xcode-select --install                 # C toolchain
brew install rustup go node git
rustup default stable && rustup component add rustfmt clippy
```

### Linux (Debian/Ubuntu)

```bash
sudo apt-get install -y build-essential pkg-config libssl-dev git curl
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
rustup component add rustfmt clippy
# Go 1.25+ from https://go.dev/dl/ and Node 22 via your preferred installer (nvm, fnm, nodesource)
```

### Clone and verify

```bash
git clone https://github.com/seanfilimon/worktree.git
cd worktree
cargo build --workspace
(cd services/server-go && go build ./...)
(cd apps/admin && npm install)
```

### Optional: Go protobuf plugins

Only needed when you change `sync.proto`:

```bash
go install google.golang.org/protobuf/cmd/protoc-gen-go@v1.36.11
go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@v1.6.2
```

---

## Workspace layout

```
crates/                     Rust workspace (Cargo.toml at the root)
  worktree-protocol/        sync.proto + generated Rust types, hashing, ids, specs/
  worktree-sdk/             local engine (library): repo, store, diff, merge, scan, remote, ipc
  worktree-cli/             `wt` binary
  worktree-bg/              `worktree-bg` daemon binary
  worktree-git/             Git import/export (libgit2)
  worktree-e2e/             end-to-end tests over the real binaries
services/server-go/         Go server: main.go + internal/{domain,meta,objstore,iam,syncsvc,httpapi,...}
apps/admin/                 React admin panel (Vite)
apps/web/                   Next.js marketing/docs site
docs/                       user and contributor docs
scripts/                    ci.sh, e2e.sh, gen-proto.sh, install.{sh,ps1}
```

Dependency direction: `worktree-protocol` ← `worktree-sdk` ← (`worktree-cli`, `worktree-bg`,
`worktree-git`); `worktree-cli` also uses `worktree-git`. The Go server depends only on the
generated `syncpb` package from `sync.proto`. Inside the server, packages communicate only through
the interfaces in `internal/domain`.

Contracts that must stay stable (changes are additive only):

- the SDK public API (`crates/worktree-sdk/src/{lib,model,store,config,repo/mod,diff,merge,scan,remote/mod,ipc}.rs`),
- `crates/worktree-protocol/proto/sync.proto`,
- `services/server-go/internal/domain/*.go`,
- the CLI surface and the REST API in [docs/ENGINEERING_PLAN.md](./docs/ENGINEERING_PLAN.md).

---

## Building

```bash
cargo build --workspace                       # debug
cargo build --release                         # target/release/{wt,worktree-bg}
(cd services/server-go && go build -o ../../target/release/worktree-server .)   # .exe on Windows
(cd apps/admin && npm run build)              # apps/admin/dist
(cd apps/web && npm run build)
```

Run a local stack for manual testing:

```bash
WT_ADMIN_PASSWORD=dev-password \
  target/release/worktree-server --data "$(mktemp -d)" --admin-dir apps/admin/dist &
export WT_HOME="$(mktemp -d)"            # keep your real ~/.wt untouched
target/release/wt login http://localhost:7420 -u admin    # password: dev-password
```

---

## Coding standards

### Rust

- Edition 2021. `cargo fmt --all` and
  `cargo clippy --workspace --all-targets -- -D warnings` must pass.
- **No `unwrap()`/`expect()` on I/O or user input in library code.** Return `SdkError` (or the
  crate's error type) with context. `unwrap` is fine in tests.
- Errors use `thiserror`; match on variants, not message strings.
- Public items get doc comments that state behavior and error cases.
- Keep `diff`, `merge` and `scan` pure (no I/O).
- Never read or write the real `~/.wt` from tests — always set `WT_HOME`.

### Go

- `gofmt` and `go vet ./...` must be clean.
- Pure Go only (no cgo). Allowed dependencies: `google.golang.org/grpc`, `google.golang.org/protobuf`,
  `go.etcd.io/bbolt`, `golang.org/x/crypto`, `golang.org/x/net`, `github.com/google/uuid`,
  `github.com/zeebo/blake3`. Anything else needs discussion first.
- Do not change existing signatures in `internal/domain`; add new ones.
- Wrap sentinel errors (`fmt.Errorf("...: %w", domain.ErrNotFound)`) so callers can use `errors.Is`.
- Table-driven tests; use `t.TempDir()` for files. Packages that depend on other packages'
  implementations test against small in-package fakes of the `domain` interfaces.

### TypeScript

- `npm run lint` (ESLint, zero warnings) and `npm run build` (includes `tsc -b`) must pass in
  `apps/admin`.
- Keep `apps/admin/src/api/types.ts` in sync with the JSON tags in `internal/domain/types.go`.
- Format with Prettier (`npm run format` at the repo root).

### Windows rules

- No hard-coded `/tmp` or `/`-only path handling; use `std::env::temp_dir()`, `tempfile`,
  `t.TempDir()`, and `Path`/`filepath` APIs. Paths stored in `.wt/` and on the wire are always
  `/`-separated.
- Atomic renames can fail with "Access is denied" when the target exists. For content-addressed
  files, treat that as success.
- Strip `\\?\` verbatim prefixes before comparing or displaying canonicalized paths.
- Expect `.exe` suffixes when locating binaries.

---

## Testing

### Unit tests

```bash
cargo test --workspace
(cd services/server-go && go test ./...)
(cd apps/admin && npm test)
```

### Integration tests (`#[ignore]` convention)

Rust tests that need something outside the process — a running server, network, a `git` binary —
are marked with an **ignore reason that starts with `integration:`**:

```rust
#[test]
#[ignore = "integration: needs a running worktree-server on WT_TEST_SERVER"]
fn push_round_trip() { /* ... */ }
```

They are skipped by default and run with:

```bash
cargo test --workspace -- --ignored
```

### End-to-end tests

`crates/worktree-e2e` drives the real `wt`, `worktree-bg` and Go server binaries (it builds the
server with `go build` on demand). Every test is gated on `WT_E2E=1`:

```bash
bash scripts/e2e.sh                       # builds everything, runs the suite
WT_E2E=1 cargo test -p worktree-e2e       # same suite, binaries already built
WT_E2E=1 WT_E2E_KEEP=1 cargo test -p worktree-e2e -- some_test   # keep temp dirs for inspection
```

Each test gets its own server data directory, `WT_HOME` and worktrees, so tests run in parallel.

### Full pipeline

```bash
bash scripts/ci.sh
```

runs formatting, lints, builds and tests for Rust, Go and the frontends — the same checks as CI.

---

## Protocol changes and code generation

1. Edit `crates/worktree-protocol/proto/sync.proto` following
   [`proto/CONVENTIONS.md`](./crates/worktree-protocol/proto/CONVENTIONS.md): additive changes only,
   wrapped ids, tags 100–199 reserved.
2. Rust bindings regenerate automatically on `cargo build` (`build.rs`, vendored `protoc`).
3. Regenerate the Go bindings and commit them:

   ```bash
   cargo build -p worktree-protocol          # makes the vendored protoc available
   bash scripts/gen-proto.sh                 # writes services/server-go/internal/proto/syncpb/
   ```

   Set `PROTOC=/path/to/protoc` to use a different compiler.
4. Update [docs/protocol-spec.md](./docs/protocol-spec.md).

---

## Documentation

- User-facing behavior changes must update the matching page in [`docs/`](./docs/README.md)
  (CLI reference, configuration, protocol, server architecture…).
- Docs describe what is implemented. Features that exist only in
  `crates/worktree-protocol/specs/` are labelled **Roadmap**.
- Code blocks must be copy-pasteable; relative links must resolve.
- Specs in `crates/worktree-protocol/specs/` are design documents; when an implementation
  deliberately deviates, note it in the spec.

---

## Commits and pull requests

### Branches

`<type>/<short-description>`, e.g. `feat/sdk-revert`, `fix/server-push-race`, `docs/cli-reference`.

### Commit messages

[Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <summary in the imperative, ≤ 72 chars>

<body: what and why, wrapped at 72>
```

- **Types**: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`, `chore`, `spec`
- **Scopes**: `protocol`, `sdk`, `cli`, `bg`, `git`, `e2e`, `server`, `admin`, `web`, `docs`, `scripts`

```
feat(sdk): add Repository::revert
fix(server): retry branch CAS on concurrent push
docs(cli): document exit code 3
```

### Pull requests

Before opening a PR:

- [ ] `bash scripts/ci.sh` passes locally (or the relevant subset for your component).
- [ ] New behavior has tests; bug fixes have a regression test.
- [ ] Docs updated (and specs, if the design changed).
- [ ] No changes to stable contracts except additive ones, called out in the description.

Describe **what** changed, **why**, **how** it was tested, and link the relevant spec or plan
section. Address review feedback with new commits — do not force-push during review. Maintainers
merge with a **merge commit** (no squash, no rebase): we practice append-only history ourselves.

---

## Review criteria

| Criterion | What we look for |
|---|---|
| Correctness | Does it do what it claims, and do the tests prove it? |
| Contract alignment | Matches the engineering plan, proto and domain interfaces; deviations are additive and documented. |
| Code quality | Idiomatic Rust/Go/TypeScript, no dead code, no TODO without an issue. |
| Tests | Edge cases covered; Windows paths considered. |
| Security | No `unwrap` on input, no secrets in code or logs, no trust in client-side checks. |
| Principles | Append-only, non-destructive, one job per command. |

---

## Issue reporting

**Bugs**: what happened, what you expected, minimal steps to reproduce, OS and tool versions
(`wt --version`, `worktree-server --version`, `rustc --version`, `go version`), and full output (run
with `WT_LOG=debug` for client-side diagnostics).

**Feature requests**: the problem, the proposed behavior, which spec covers it (if any), and
alternatives considered.

---

## Security vulnerabilities

**Do not open a public issue for security vulnerabilities.** Report them privately to the
maintainers (contact [@seanfilimon](https://github.com/seanfilimon)) with a description, steps to
reproduce, impact, and a suggested fix if you have one. We acknowledge reports within 48 hours and
agree on a disclosure timeline with you. See [docs/security.md](./docs/security.md) for the current
security model.

---

## License

By contributing you agree that your contributions are licensed under the **W0rkTree Public License
v1.0** ([`LICENSE`](./LICENSE)) — the GNU GPL v2 plus a Brand Protection clause (Section 11). Your
code stays open source; nobody may strip the W0rkTree name and rebrand the codebase.

### Developer Certificate of Origin

By submitting a contribution you certify that (1) you created it and have the right to submit it
under the W0rkTree Public License v1.0; or (2) it is based on previous work under a compatible
open-source license and you have the right to submit it with modifications; or (3) it was provided to
you by someone who certified (1) or (2) and you have not modified it.

---

## Code of conduct

Be professional and respectful. Give direct, actionable review feedback; disagree on technical merits
with evidence; help newcomers; credit others' work. Personal attacks, dismissing contributions without
explanation, gatekeeping and submitting others' work as your own are not tolerated. Maintainers
address violations; repeated violations lead to removal from the project.

---

<div align="center">

**W0rkTree is not the next version of Git. It is what comes after Git.**

*Build it with us.*

</div>
