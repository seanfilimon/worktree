# Worktree Server Architecture

The Worktree server is a long-running daemon that watches the filesystem, automatically tracks changes, and manages the lifecycle of trees, snapshots, and branches. It provides the core engine that powers Worktree's automatic version control capabilities.

## Daemon Lifecycle

TODO: Document how the server daemon starts, stops, and manages its lifecycle. Cover process management, signal handling, graceful shutdown, and crash recovery.

## Filesystem Watcher

TODO: Document the filesystem watching subsystem. Cover platform-specific backends (inotify, FSEvents, ReadDirectoryChangesW), debouncing strategies, ignore patterns, and performance considerations for large trees.

## Event Engine

TODO: Document the event processing pipeline. Cover how filesystem events are collected, filtered, batched, and dispatched to downstream consumers. Include event types, ordering guarantees, and backpressure handling.

## Auto-Commit Engine

TODO: Document the automatic commit (snapshot) system. Cover triggers for automatic snapshots, debounce intervals, content-based deduplication, and configuration options for snapshot frequency and granularity.

## Auto-Branch Engine

TODO: Document automatic branch management. Cover heuristics for detecting logical branches of work, branch naming strategies, automatic branch switching, and integration with the snapshot engine.

## Storage Backend

TODO: Document the storage layer. Cover content-addressable storage, object packing, garbage collection, compression strategies, and the on-disk format. Include details on how nested trees share storage.

## API Surface

TODO: Document the server's API for clients. Cover the IPC mechanism (Unix sockets, named pipes), request/response protocol, streaming events, authentication, and the command set exposed to the CLI and SDK.