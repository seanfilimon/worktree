# Worktree CLI Reference

The Worktree CLI (`wt`) provides a comprehensive command-line interface for interacting with Worktree repositories. It is designed to be intuitive for developers familiar with version control systems while exposing the full power of Worktree's nested tree architecture.

## Commands

### `wt init`

Initialize a new Worktree tree in the current directory.

```
wt init [path]
```

Creates a new Worktree repository, setting up the necessary metadata and starting the background server daemon. If `path` is provided, the tree is initialized at that location instead of the current directory.

---

### `wt status`

Show the current tree status.

```
wt status
```

Displays the state of the working tree, including modified files, pending snapshots, active branch, and nested tree status.

---

### `wt snapshot`

Create a snapshot of the current tree state.

```
wt snapshot [--message <msg>]
```

Records the current state of all tracked files as an immutable snapshot. In most configurations, snapshots are created automatically by the server daemon, but this command allows manual snapshot creation when needed.

---

### `wt branch`

Manage branches.

```
wt branch create <name>       Create a new branch
wt branch list                 List all branches
wt branch switch <name>        Switch to an existing branch
wt branch delete <name>        Delete a branch
```

Branches in Worktree are lightweight references to snapshot chains. The auto-branch engine may also create branches automatically based on configured rules.

---

### `wt merge`

Merge branches.

```
wt merge <source> [--into <target>]
```

Merges the specified source branch into the target branch (defaults to the current branch). Worktree uses semantic merge strategies that understand nested tree boundaries.

---

### `wt log`

View snapshot history.

```
wt log [--limit <n>] [--branch <name>] [--format <format>]
```

Displays the history of snapshots for the current or specified branch. Supports various output formats for integration with other tools.

---

### `wt sync`

Push and pull changes to/from remote repositories.

```
wt sync push [<remote>]        Push local snapshots to a remote
wt sync pull [<remote>]        Pull remote snapshots to local
```

Synchronizes snapshot history between local and remote Worktree repositories. Supports both Worktree-native remotes and Git-compatible remotes.

---

### `wt tree`

Manage nested trees.

```
wt tree add <path> [<url>]     Add a nested tree
wt tree list                   List all nested trees
wt tree remove <path>          Remove a nested tree
```

Nested trees are a core concept in Worktree, allowing repositories to contain other repositories with independent versioning and permissions.

---

### `wt permission`

Manage permissions on trees and subtrees.

```
wt permission set <path> <rule>    Set a permission rule
wt permission get <path>           Get permissions for a path
wt permission list                 List all permission rules
```

Worktree supports fine-grained permission control at the tree and subtree level, enabling multi-tenant and enterprise workflows.

---

### `wt git`

Git interoperability commands.

```
wt git import <git-repo>       Import a Git repository into Worktree
wt git export <path>           Export a Worktree tree as a Git repository
wt git clone <git-url>         Clone a Git repository as a Worktree tree
wt git remote <args>           Manage Git remote bridges
wt git push [<remote>]         Push to a Git remote
wt git pull [<remote>]         Pull from a Git remote
wt git mirror <git-url>        Set up live mirror with a Git repository
```

Full bidirectional Git compatibility, allowing Worktree to interoperate seamlessly with existing Git infrastructure.

---

### `wt server`

Manage the Worktree background server daemon.

```
wt server start                Start the server daemon
wt server stop                 Stop the server daemon
wt server status               Show server daemon status
```

The server daemon handles filesystem watching, automatic snapshotting, automatic branching, and serves the local API for CLI and SDK access.