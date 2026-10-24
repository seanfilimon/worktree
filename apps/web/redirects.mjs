/**
 * Permanent redirects from documentation URLs that existed before the v0.2.0
 * rebuild to the page that now covers the same topic. Keeps external links
 * and bookmarks working after the docs restructure.
 *
 * @type {ReadonlyArray<readonly [source: string, destination: string]>}
 */
const moved = [
  // CLI reference: one page per command group now.
  ["/docs/cli/access", "/docs/server/access-control"],
  ["/docs/cli/archive", "/docs/cli/snapshots#wt-archive"],
  ["/docs/cli/environment", "/docs/cli"],
  ["/docs/cli/git-compatibility", "/docs/cli/git"],
  ["/docs/cli/license", "/docs/server/licensing"],
  ["/docs/cli/merge-request", "/docs/server/branch-protection"],
  ["/docs/cli/staged", "/guides/staged-visibility"],
  ["/docs/cli/tags-releases", "/docs/cli/branches#wt-tag"],
  ["/docs/cli/tenant", "/docs/server/access-control"],
  ["/docs/cli/tree-management", "/docs/cli/repository#wt-tree"],
  ["/docs/cli/version-control", "/docs/cli/snapshots"],
  ["/docs/cli/worker", "/docs/cli/daemon"],

  // IAM and licensing folded into the server docs.
  ["/docs/iam", "/docs/server/access-control"],
  ["/docs/iam/:slug*", "/docs/server/access-control"],
  ["/docs/licensing", "/docs/server/licensing"],
  ["/docs/licensing/:slug*", "/docs/server/licensing"],

  // Protocol.
  ["/docs/protocol/diff-semantics", "/docs/protocol/merge-and-diff"],
  ["/docs/protocol/merge-semantics", "/docs/protocol/merge-and-diff"],
  ["/docs/protocol/iam-protocol", "/docs/server/access-control"],
  ["/docs/protocol/snapshot-format", "/docs/protocol/object-model"],
  ["/docs/protocol/storage-model", "/docs/protocol/object-model"],
  ["/docs/protocol/wire-format", "/docs/protocol/object-model"],
  ["/docs/protocol/tree-structure", "/docs/protocol/local-layout"],
  ["/docs/protocol/staged-visibility-protocol", "/docs/protocol/sync-protocol"],
  ["/docs/protocol/transport", "/docs/protocol/sync-protocol"],
  ["/docs/protocol/versioning", "/docs/protocol"],

  // SDK.
  ["/docs/sdk/installation", "/docs/sdk"],
  ["/docs/sdk/connecting", "/docs/sdk/remote"],
  ["/docs/sdk/staged-operations", "/docs/sdk/remote"],
  ["/docs/sdk/tree-operations", "/docs/sdk/repository"],
  ["/docs/sdk/snapshot-operations", "/docs/sdk/repository"],
  ["/docs/sdk/branch-operations", "/docs/sdk/repository"],
  ["/docs/sdk/tenant-operations", "/docs/server/rest-api"],
  ["/docs/sdk/access-operations", "/docs/server/rest-api"],
  ["/docs/sdk/license-operations", "/docs/server/rest-api"],

  // Server.
  ["/docs/server/api-surface", "/docs/server/rest-api"],
  ["/docs/server/architecture", "/docs/server"],
  ["/docs/server/bgprocess", "/docs/cli/daemon"],
  ["/docs/server/event-pipeline", "/docs/server/events"],
  ["/docs/server/iam-enforcement", "/docs/server/access-control"],
  ["/docs/server/license-enforcement", "/docs/server/licensing"],
  ["/docs/server/merge-requests", "/docs/server/branch-protection"],
  ["/docs/server/staged-snapshots", "/guides/staged-visibility"],
  ["/docs/server/tenants", "/docs/server/access-control"],

  // Guides.
  ["/guides/admin", "/guides/admin-panel"],
  ["/guides/admin/:slug*", "/guides/admin-panel"],
  ["/guides/archiving", "/guides/tags-and-releases"],
  ["/guides/versioning", "/guides/tags-and-releases"],
  ["/guides/declarative-access", "/guides/multi-tenancy"],
  ["/guides/permissions-and-acl", "/guides/multi-tenancy"],
  ["/guides/security-deep-dive", "/docs/security"],
  ["/guides/sync-protocol", "/docs/protocol/sync-protocol"],
  ["/guides/ci-cd-integration", "/roadmap"],
  ["/guides/dependency-management", "/roadmap"],
  ["/guides/large-files", "/roadmap"],
  ["/guides/linked-branches", "/roadmap"],
  ["/guides/shallow-history", "/roadmap"],
];

export const legacyDocRedirects = moved.map(([source, destination]) => ({
  source,
  destination,
  permanent: true,
}));
