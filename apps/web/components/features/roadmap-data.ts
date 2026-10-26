/**
 * The roadmap's milestones, shared by the roadmap page and the features page's "not shipped yet" chapter so the
 * two never disagree about what is planned.
 */

export interface Milestone {
  /** The release a shipped milestone went out in ("v0.2", "v0.3–v0.4"); planned work has none. */
  version?: string;
  title: string;
  status: 'shipped' | 'planned';
  summary: string;
  items: string[];
}

export const milestones: Milestone[] = [
  {
    version: 'v0.2',
    title: 'the v2 rebuild',
    status: 'shipped',
    summary:
      'A ground-up rebuild around one binding contract: a Rust client stack, a Go server on a single port and a React admin panel.',
    items: [
      'wt CLI for snapshots, history, diff, branches, merge, tags, nested trees, ignore rules and config',
      'worktree-bg daemon with file watching, auto-snapshots and auto-staging',
      'Go server: gRPC sync + REST + SSE + admin panel on one port',
      'Tenants, roles, tree visibility, staged snapshots, branch protection, merge requests, license rules, audit log',
      'Secret scanning and Git import/export',
    ],
  },
  {
    version: 'v0.3–v0.4',
    title: 'admin panel and IAM',
    status: 'shipped',
    summary: 'A full admin panel and a complete identity and access system.',
    items: [
      'Monochrome shadcn/ui admin panel for trees, history, files, merge requests and operations',
      'Teams, custom roles, deny-overrides policies with conditions, per-tree grants, service accounts',
      'Scoped tokens, TOTP two-factor sign-in, sessions, invitations, OpenID Connect SSO',
      'A wt command for everything the server can do',
    ],
  },
  {
    version: 'v0.5',
    title: 'protocol completeness',
    status: 'shipped',
    summary: 'Every protocol spec either built or explicitly on the roadmap.',
    items: [
      'Chunked large files, shallow clones and license-restricted paths',
      'CI checks, stale-review rules, linked merge groups, tag protection',
      'Snapshot dependencies, TODOs and releases; Ed25519 snapshot signing',
      'Declarative access files (wt access plan/apply), tenant suspension, quotas, rate limits, TLS, garbage collection',
    ],
  },
  {
    version: 'v0.6',
    title: 'collaboration and tooling',
    status: 'shipped',
    summary: 'Server-side merges and the everyday tools of a code host.',
    items: [
      'Merge, squash and fast-forward merges on the server with conflict reporting',
      'Webhooks, code search, blame, policy history',
      'Cherry-pick, rebase and stash; zstd-compressed storage with gc and fsck',
      'Shell completions, wt doctor, aliases, an OpenAPI 3.1 description and wt api',
    ],
  },
  {
    version: 'v0.7',
    title: 'deployable and team-ready',
    status: 'shipped',
    summary: 'Run it in production and live in it every day.',
    items: [
      'Docker images, Compose, a Helm chart, systemd/launchd units, installers and release builds',
      'S3-compatible object storage, scheduled backups and offline restore',
      'Notifications with watch levels and email',
      'Sparse checkout and partial clones; wt sync with an offline push queue',
      'Live Git push mirrors; path owners, draft merge requests and suggested changes',
      'Signed tags, release checksums and SBOMs; persistent code search with symbols',
    ],
  },
  {
    version: 'v0.8',
    title: 'scale, governance and safety nets',
    status: 'shipped',
    summary: 'Faster transfer, forks, configuration in the tree and a stronger access model.',
    items: [
      'Pack files with binary deltas for push, fetch and clone, resumable uploads',
      'Forks across tenants and merge requests from forks',
      'Config-as-code: branches and protections declared in .wt-tree/, validated on push',
      'Tenant email verification, slug renames, deletion with a recovery window',
      'ABAC attributes in policy conditions; device sign-in and rotating refresh tokens',
      'Server-side secret scanning and license audit; resolving conflicts in the admin panel and rebase merges',
      'Force push with leases, protection bypass, wt undo/redo; labeled staged snapshots',
      'Backups to S3, a durable email outbox and retention settings',
    ],
  },
  {
    title: 'Distribution',
    status: 'planned',
    summary: 'Publish what the release pipeline already builds.',
    items: [
      'Published release archives and container images',
      'Package-manager installs (Homebrew, winget, crates.io)',
    ],
  },
  {
    title: 'Scale and transport',
    status: 'planned',
    summary: 'Bigger repositories and more servers.',
    items: [
      'QUIC transport alongside HTTP/2',
      'Virtual filesystem with lazy loading (FUSE / ProjFS)',
      'High availability and a clustered metadata store',
    ],
  },
  {
    title: 'Workflows',
    status: 'planned',
    summary: 'Richer history editing and multi-tree automation.',
    items: [
      'Interactive rebase',
      'Bidirectional Git mirroring',
      'Pull mirrors (import from Git on a schedule)',
    ],
  },
  {
    title: 'Security',
    status: 'planned',
    summary: 'Stronger guarantees for regulated environments.',
    items: ['Encryption at rest', 'Branch-scoped role assignment', 'Physical tenant isolation'],
  },
];

export const shippedMilestones = milestones.filter((milestone) => milestone.status === 'shipped');
export const plannedMilestones = milestones.filter((milestone) => milestone.status === 'planned');
