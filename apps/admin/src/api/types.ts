/**
 * Wire types for the W0rkTree REST API (/api/v1).
 *
 * Field names mirror the `json` tags in
 * services/server-go/internal/domain/types.go exactly. Timestamps are RFC 3339
 * strings; hashes are lowercase hex BLAKE3-256 digests.
 */

export type ISODateString = string;

export const ROLES = ["owner", "admin", "maintainer", "developer", "viewer"] as const;
export type Role = (typeof ROLES)[number];

export type TenantKind = "user" | "org";
export type TenantPlan = "free" | "pro" | "enterprise";

export const VISIBILITIES = ["private", "internal", "public"] as const;
export type Visibility = (typeof VISIBILITIES)[number];

export const MR_STATES = ["open", "merged", "closed"] as const;
export type MergeRequestState = (typeof MR_STATES)[number];

export const LICENSE_GRANTS = ["read-only", "modify", "redistribute"] as const;
export type LicenseGrant = (typeof LICENSE_GRANTS)[number];

export interface Tenant {
  id: string;
  slug: string;
  name: string;
  kind: TenantKind;
  plan: TenantPlan;
  created_at: ISODateString;
}

export interface Account {
  id: string;
  username: string;
  email: string;
  display_name: string;
  tenant_id: string;
  is_admin: boolean;
  disabled: boolean;
  created_at: ISODateString;
}

export interface Membership {
  tenant_id: string;
  account_id: string;
  role: Role;
  created_at: ISODateString;
}

export interface APIToken {
  id: string;
  account_id: string;
  name: string;
  created_at: ISODateString;
  expires_at?: ISODateString;
  last_used_at?: ISODateString;
}

export interface Tree {
  id: string;
  tenant_id: string;
  name: string;
  description: string;
  default_branch: string;
  visibility: Visibility;
  created_at: ISODateString;
  updated_at: ISODateString;
}

export interface Branch {
  tree_id: string;
  name: string;
  /** Snapshot id; "" for an empty branch. */
  tip: string;
  protected: boolean;
  created_at: ISODateString;
  updated_at: ISODateString;
}

export interface Snapshot {
  id: string;
  tree_id: string;
  branch_name: string;
  parents: string[] | null;
  manifest_hash: string;
  author: string;
  message: string;
  created_at: ISODateString;
  auto_generated: boolean;
  /** Account id of the pusher. */
  pushed_by: string;
  pushed_at: ISODateString;
}

export interface Tag {
  tree_id: string;
  name: string;
  /** Snapshot id. */
  target: string;
  message: string;
  tagger: string;
  created_at: ISODateString;
}

export interface StagedSnapshot {
  snapshot_id: string;
  tree_id: string;
  tenant_id: string;
  account_id: string;
  author: string;
  branch_name: string;
  files_changed: string[] | null;
  files_added: number;
  files_modified: number;
  files_deleted: number;
  message: string;
  staged_at: ISODateString;
}

export interface BranchProtection {
  tree_id: string;
  /** path.Match glob, e.g. "main" or "release/*". */
  pattern: string;
  no_direct_push: boolean;
  required_approvals: number;
  /** Usernames; empty = role-based only. */
  allowed_pushers: string[] | null;
  no_delete: boolean;
}

export interface Approval {
  account_id: string;
  username: string;
  at: ISODateString;
}

export interface MergeRequest {
  id: string;
  tree_id: string;
  number: number;
  title: string;
  description: string;
  source_branch: string;
  target_branch: string;
  author_id: string;
  author: string;
  state: MergeRequestState;
  approvals: Approval[] | null;
  created_at: ISODateString;
  updated_at: ISODateString;
  merged_at?: ISODateString;
  merged_snapshot?: string;
}

export interface AuditEvent {
  id: string;
  tenant_id: string;
  actor_id: string;
  /** Username, "anonymous" or "system". */
  actor: string;
  action: string;
  resource_type: string;
  resource_id: string;
  details?: Record<string, string>;
  remote_addr?: string;
  at: ISODateString;
}

export interface LicenseRule {
  tree_id: string;
  path_glob: string;
  spdx: string;
  grant: LicenseGrant;
}

export const EVENT_TYPES = [
  "snapshot.staged",
  "staged.cleared",
  "branch.pushed",
  "branch.created",
  "tag.pushed",
  "tree.created",
  "mr.opened",
  "mr.approved",
  "mr.merged",
  "mr.closed",
] as const;
export type KnownEventType = (typeof EVENT_TYPES)[number];

/** A domain.Event streamed over SSE. */
export interface ServerEvent {
  type: string;
  tenant_id: string;
  tree_id?: string;
  actor?: string;
  payload?: unknown;
  at: ISODateString;
}

export interface ServerStats {
  tenants: number;
  accounts: number;
  trees: number;
  branches: number;
  snapshots: number;
  staged: number;
  objects: number;
  storage_bytes: number;
}

// ---- Composite response shapes (§7) ----

export interface Health {
  status: string;
  version: string;
  uptime_seconds: number;
  /** Not part of the v2 contract; honoured when a server reports it. */
  allow_registration?: boolean;
}

export interface AuthResponse {
  token: string;
  account: Account;
}

export interface MeResponse {
  account: Account;
  memberships: { tenant: Tenant; role: Role }[] | null;
}

export interface CreatedToken {
  /** The raw bearer token. Only ever returned once. */
  token: string;
  api_token: APIToken;
}

export interface Member {
  account: Account;
  role: Role;
}

export interface TreeSummary {
  tree: Tree;
  branch_count: number;
  snapshot_count: number;
  staged_count: number;
  open_mr_count: number;
}

export interface SnapshotFile {
  path: string;
  hash: string;
  size: number;
  executable: boolean;
}

export interface SnapshotDetail {
  snapshot: Snapshot;
  files: SnapshotFile[] | null;
}

// ---- Request bodies ----

export interface RegisterRequest {
  username: string;
  email: string;
  password: string;
}

export interface LoginRequest {
  username: string;
  password: string;
}

export interface CreateTokenRequest {
  name: string;
  /** 0 = never expires. */
  ttl_hours: number;
}

export interface CreateTenantRequest {
  slug: string;
  name: string;
}

export interface CreateTreeRequest {
  name: string;
  description: string;
  visibility: Visibility;
  default_branch: string;
}

export interface UpdateTreeRequest {
  description?: string;
  visibility?: Visibility;
  default_branch?: string;
}

export interface CreateMergeRequestRequest {
  title: string;
  description: string;
  source_branch: string;
  target_branch: string;
}

export interface UpdateAccountRequest {
  is_admin?: boolean;
  disabled?: boolean;
}

export interface AuditQuery {
  tenant?: string;
  action?: string;
  limit?: number;
}
