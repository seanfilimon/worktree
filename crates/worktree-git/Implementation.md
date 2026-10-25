# Implementation Details — `worktree-git`

Git compatibility layer for W0rkTree: converts Git commit DAGs into
snapshots (`wt git import`) and snapshots back into Git commits
(`wt git export`), on top of `worktree-sdk` and libgit2 (`git2`).

## Layout

```
src/
├── lib.rs            public modules + re-exports
├── bridge.rs         contract: ImportOptions/Report, ExportOptions/Report,
│                     import_repository, export_repository (+ end-to-end tests)
├── import.rs         Git -> W0rkTree (TreeConverter, branch walk, tags, ignore)
├── export.rs         W0rkTree -> Git (TreeWriter, deterministic commits, tags)
├── license.rs        license rules (glob/SPDX ports of the Go server), cache in
│                     .wt/trees/<tree>/licenses.json, redistribution filter,
│                     generated LICENSES/NOTICE
├── source.rs         import sources: local path, or URL mirrored to a temp dir
├── map.rs            GitMap: .wt/git-map.json (snapshot id <-> commit sha)
├── ident.rs          UUID v5 snapshot ids, author <-> signature, messages, time
├── error.rs          GitCompatError + Result
├── hash_index/       InMemoryHashIndex (BLAKE3 <-> SHA-1), export blob cache
├── config/
│   ├── gitignore.rs      .gitignore -> .wt/ignore pattern extraction
│   └── gitattributes.rs  .gitattributes parser, Git LFS detection
├── remote/
│   ├── mod.rs        push (local Git repo -> URL), fetch_mirror (URL -> bare mirror)
│   ├── auth.rs       GitAuth (auto / SSH key / credential helper callbacks)
│   └── transport.rs  GitTransport (HTTPS / SSH / local detection)
└── test_support.rs   (tests only) Git fixtures and in-memory blob store
```

## Import

1. Open the Git repository; select local branches (all, or the requested
   ones — unknown names fail before anything is created).
2. `Repository::init(dest)` with the default branch = Git HEAD's branch (if
   selected, else the first selected branch by name).
3. Walk all selected tips with `TOPOLOGICAL | REVERSE` (parents first; shared
   commits converted once). Each commit is attributed to the first branch
   containing it (checked-out branch first, then by name).
4. Per commit: blobs -> object store (each Git blob once), manifest (sorted
   paths, `100755` = executable; symlinks/gitlinks skipped with a warning;
   cached per Git tree), snapshot with id = UUID v5(commit sha), parents
   mapped, author `Name <email>`, committer time, message without trailing
   whitespace.
5. Branch tips, tags (annotated keep message/tagger/time; tags outside the
   imported history are skipped), `.wt/git-map.json`, forced checkout of the
   default branch, root `.gitignore` patterns -> `.wt/ignore`, warning for
   Git LFS patterns from `.gitattributes`.

## Export

1. Open the target Git repository or create it (bare if requested) with
   HEAD -> the tree's current branch.
2. For each branch tip: if the tip is mapped and the commit exists in the
   target, nothing to do; otherwise walk `ancestry(tip)` and build missing
   commits: manifest -> nested Git trees (file modes, cached by manifest
   hash; blobs cached by BLAKE3), author = committer = parsed snapshot author
   at the snapshot time (UTC), message + trailing newline. Commits are
   written via `commit_create_buffer` so only new objects are counted.
3. `refs/heads/<branch>` is force-updated; tags become lightweight or
   annotated (when they have a message) refs.
4. New mappings are added to `.wt/git-map.json` (existing ones are kept, so
   imported commits keep pointing at their original SHAs).
5. A non-bare target's working tree is checked out to the new HEAD only if
   it had no local modifications.

Exports are deterministic: the same snapshots produce the same commit SHAs
in any target repository.

## Testing

Pure tests (Git fixtures built with git2, in-memory blob store) run by
default. End-to-end tests in `bridge.rs` need the real SDK engine and are
marked `#[ignore = "integration: needs sdk-core"]`; run them with
`cargo test -p worktree-git -- --ignored`. Tests set `WT_HOME` to a temp dir.
