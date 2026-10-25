import { ROLES, type Role } from "@/api/types";

/** What each role adds on top of the roles below it (mirrors internal/iam/authz.go). */
export const ROLE_SUMMARY: Record<Role, string> = {
  viewer: "Read trees, history, files and merge requests.",
  developer: "Push and stage work, create branches, open merge requests.",
  maintainer: "Create trees, delete branches, approve and merge merge requests.",
  admin: "Administer trees and the tenant: members, protections, licenses, settings.",
  owner: "Everything, including granting and revoking the owner role.",
};

export interface Capability {
  label: string;
  /** Least privileged role granting it. */
  minRole: Role;
}

export const CAPABILITIES: Capability[] = [
  { label: "Read trees & history", minRole: "viewer" },
  { label: "Push, stage, open MRs", minRole: "developer" },
  { label: "Create branches", minRole: "developer" },
  { label: "Approve & merge MRs", minRole: "maintainer" },
  { label: "Create trees, delete branches", minRole: "maintainer" },
  { label: "Tree settings & protections", minRole: "admin" },
  { label: "Manage members & tenant", minRole: "admin" },
  { label: "Grant or revoke owner", minRole: "owner" },
];

/** Roles ordered from least to most privileged. */
export const ROLES_ASCENDING: readonly Role[] = [...ROLES].reverse();

export function roleRank(role: Role): number {
  return ROLES_ASCENDING.indexOf(role);
}

export function roleAllows(role: Role, minRole: Role): boolean {
  return roleRank(role) >= roleRank(minRole);
}
