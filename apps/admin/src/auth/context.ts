import { createContext, useContext } from "react";
import type { UseQueryResult } from "@tanstack/react-query";
import type { Account, AuthResponse, MeResponse, Role } from "@/api/types";

export interface AuthContextValue {
  token: string | null;
  me: UseQueryResult<MeResponse>;
  account: Account | undefined;
  isAdmin: boolean;
  /** Stores a token returned by login/register and starts the session. */
  signIn: (res: AuthResponse) => void;
  /** Revokes the current token server-side (best effort) and signs out. */
  signOut: () => Promise<void>;
  /** The caller's role in a tenant (by id), if any. */
  roleIn: (tenantId: string) => Role | undefined;
  /** Message from a failed SSO round trip (`#sso_error=`), until dismissed. */
  ssoError: string | null;
  clearSsoError: () => void;
}

export const AuthContext = createContext<AuthContextValue | null>(null);

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used inside <AuthProvider>");
  return ctx;
}

/**
 * Roles that can manage tenant members. UI gating only: the server is the
 * authority and rejects anything the caller may not do.
 */
export function canManage(role: Role | undefined, isAdmin: boolean): boolean {
  return isAdmin || role === "owner" || role === "admin";
}

/** Roles that can administer a tree (protections, licenses, settings). */
export function canMaintain(role: Role | undefined, isAdmin: boolean): boolean {
  return canManage(role, isAdmin) || role === "maintainer";
}

/** Only follow same-app relative redirects. */
export function safeRedirect(from: unknown): string {
  return typeof from === "string" && from.startsWith("/") && !from.startsWith("//") ? from : "/";
}
