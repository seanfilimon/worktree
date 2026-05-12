# W0rkTree Roadmap (Demo vs. Production)

## Purpose

This roadmap defines the path to delivering a fully functional, high-impact **Demo Version** to prove the viability of the W0rkTree architecture, while simultaneously maintaining a strict backlog of architectural changes required for the **Production-Ready Service**.

The immediate goal is to finalize the Rust-based local prototype (`worktree-server` and `worktree-sdk`) for the demo. Following the demo, development will shift to building the authoritative, multi-tenant Go server for production.

---

## Part 1: The "Killer Demo" Milestones (Immediate Priority)

These are the critical fixes and missing features required to make the local Rust prototype function flawlessly for the engineering team lead presentation.

### 1. Staged Snapshot Visibility (The "Killer Feature")
* **Goal:** Demonstrate real-time team visibility of unpushed work.
* **To Do:**
  * Add the missing `/staged/ws` endpoint to the Axum router.
  * Implement WebSocket connection upgrading in the server.
  * Wire up a `tokio::sync::broadcast` or `mpsc` channel to forward incoming HTTP `POST /staged` payloads to all connected WebSocket clients dynamically.

### 2. Snapshot-Based Conflict Resolution (Fixing the "Revert Bug")
* **Goal:** Prove the 3-way snapshot diffing algorithm works end-to-end without data loss.
* **To Do:**
  * **Critical Fix:** Modify `worktree-sdk/src/engine/merge.rs` to write the `merged_files` directly to the physical disk (working directory) *before* saving the database state. Currently, the disconnected filesystem state triggers the `bgprocess` to auto-snapshot and silently revert the merge.

### 3. IAM Enforcement Demo (Securing the Middleware)
* **Goal:** Show that Role-Based/Attribute-Based Access Control works without exposing the system to crashes.
* **To Do:**
  * **Critical Fix:** Remove the severe Out-of-Memory (OOM) vulnerability in `require_auth` middleware (`axum::body::to_bytes(body, usize::MAX)`).
  * Refactor the client and server to pass `tree_id` via HTTP headers (e.g., `x-wt-tree-id`) or URL path parameters (e.g., `/api/tree/:id/snapshot`) to avoid parsing the JSON body during auth enforcement.

### 4. Resilient Background Sync Loop
* **Goal:** Show that the local watcher can reliably push to the remote without leaking memory or dropping changes.
* **To Do:**
  * Add a bounded capacity to the `push_tx` channel (`mpsc::sync_channel` or `tokio::sync::mpsc`) to prevent unbounded RAM growth.
  * Add basic retry logic to the background thread if a `push_staged` network request fails.

---

## Part 2: Production-Ready Service (Go Rewrite & System Hardening)

Once the demo is approved, the remote server must be rebuilt as a stateless Go service. The Rust prototype mixes local bgprocess behavior with remote server handlers, which violates the strict two-runtime architecture spec.

### Target Production Architecture
* **Language:** Go (`wt-server-go`)
* **Databases:** PostgreSQL (Tenants, IAM, Branches, Quotas), S3-compatible Object Storage (BLAKE3 CAS)
* **Protocols:** gRPC (migrating to QUIC), REST, WebSocket

### Production System Hardening
Based on the architectural review of the prototype, the following must be implemented in the production systems:

1. **Graph-Aware Sync Traversal:** 
   * Replace the linear array-slicing logic in `push_unpushed()` with a true Directed Acyclic Graph (DAG) traversal to ensure merged historical branches are properly synced.
2. **Robust Sync WAL:** 
   * Implement a persistent Write-Ahead Log (WAL) on the local client for unpushed snapshots to survive system restarts without losing track of network retries.
3. **Advanced LCA Merge Algorithm:** 
   * Evolve the Breadth-First Search (BFS) MRCA discovery to a timestamp-aware Lowest Common Ancestor (LCA) traversal to handle highly complex, criss-cross Git-style merges.
4. **Compiled Policy Evaluation:** 
   * Pre-compile `.wt/access/*.toml` files into a relational database representation upon push for O(1) server-side lookup speeds.

### Production Feature Roadmap

* **Phase 1: Canonical Storage & Auth:** Fully isolated Go service storing append-only history in Postgres/S3. Implement strict JWT/mTLS authentication and ABAC policy evaluation.
* **Phase 2: Large File Handling (FastCDC):** Replace fixed-size chunking with FastCDC algorithms and deduplication. Implement lazy loading via Virtual Filesystems (FUSE/ProjFS).
* **Phase 3: Cross-Tree Dependencies:** Implement atomic merges for multi-repo linked dependencies (e.g., synchronizing `frontend/feature-x` and `backend/feature-x`).
* **Phase 4: License Compliance Engine:** Enforce SPDX file-level controls to prevent proprietary code leakage across tenant boundaries.
* **Phase 5: Git Bridge & Mirroring:** Bidirectional Git conversion to allow gradual, risk-free adoption (`wt git mirror`).
  * *Deferred CLI Integration:* The CLI commands (`wt git import`, `wt git export`) currently only print mock progress messages. Wiring the `worktree-git` libgit2 DAG traversal and disk persistence into the `worktree-cli` is deferred until after native network stability is finalized.
  * *Required Architecture Improvement:* The current `worktree-git` converter stubs load entire objects into memory and use deterministic hashes to mock Account IDs. Production implementation must stream large Git blobs to avoid OOM crashes, support Git LFS, and query the IAM database to accurately map Git commit emails to real W0rktree `AccountId`s (and vice-versa).