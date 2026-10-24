# Git Compatibility Guide

Worktree provides bidirectional Git compatibility, allowing seamless interoperability between Worktree repositories and existing Git workflows. This guide covers the mechanisms for importing, exporting, and maintaining live synchronization between Worktree trees and Git repositories.

## Importing from Git

TODO: Document the process of importing an existing Git repository into a Worktree tree, including history conversion, branch mapping, and handling of Git-specific features (submodules, LFS, etc.).

## Exporting to Git

TODO: Document how to export a Worktree tree back to a standard Git repository, including snapshot-to-commit mapping, branch reconstruction, and metadata preservation.

## Git Remote Bridge

TODO: Document the Git remote helper that allows Worktree to act as a Git remote, enabling standard Git clients to push to and pull from Worktree servers using familiar Git commands.

## Live Mirror Mode

TODO: Document the live mirror capability that maintains a real-time synchronized Git repository alongside a Worktree tree, automatically reflecting changes in both directions.

## Object Mapping

TODO: Document the mapping between Worktree objects (nodes, snapshots, trees) and Git objects (blobs, commits, trees), including hash translation tables and reference management.

## Round-Trip Guarantee

TODO: Document the guarantees provided for round-trip fidelity — importing a Git repository into Worktree and exporting it back should produce an identical (or semantically equivalent) Git repository, preserving commit hashes where possible.