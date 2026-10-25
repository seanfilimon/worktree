/**
 * v0.4 account security endpoints (docs/ENGINEERING_PLAN.md §12.2, §12.3 and
 * [iam-sso]): sign-in providers, the MFA login step, TOTP enrollment,
 * sessions, scoped API tokens, invitation preview / accept, password resets
 * and the admin SSO configuration. JSON shapes mirror
 * services/server-go/internal/domain/iam.go.
 */
import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import type { ApiClient } from "./client";
import { useApi } from "./context";
import type { APIToken, AuthResponse, CreateTokenRequest, ISODateString, Role, Tenant } from "./types";

const V1 = "/api/v1";
const seg = encodeURIComponent;

// ---- Types ----

/** GET /auth/providers (public). */
export interface AuthProviders {
  password: boolean;
  registration: boolean;
  sso: { enabled: boolean; name: string };
}

/** First login step when the account has a second factor. */
export interface MfaChallenge {
  mfa_required: true;
  mfa_token: string;
  account: { username: string };
}

export type LoginResponse = AuthResponse | MfaChallenge;

export function isMfaChallenge(res: LoginResponse): res is MfaChallenge {
  return "mfa_required" in res && res.mfa_required;
}

export interface MfaState {
  account_id: string;
  totp_enabled: boolean;
  enabled_at?: ISODateString;
  recovery_codes_left: number;
}

export interface TotpEnrollment {
  /** Base32 secret for manual entry. */
  secret: string;
  otpauth_url: string;
}

export interface RecoveryCodes {
  recovery_codes: string[];
}

export type TokenKind = "session" | "api" | "service";

/** APIToken with the v0.4 fields. */
export interface SecureToken extends APIToken {
  kind?: TokenKind;
  service_account_id?: string;
  scopes?: string[];
  mfa?: boolean;
  last_ip?: string;
  user_agent?: string;
}

export interface Session extends SecureToken {
  /** The session of this browser. */
  current: boolean;
}

export interface CreateScopedTokenRequest extends CreateTokenRequest {
  /** Omitted (or empty) = the token acts with all of the owner's permissions. */
  scopes?: string[];
}

export interface CreatedScopedToken {
  token: string;
  api_token: SecureToken;
}

/** GET /iam/actions (public). */
export interface IamAction {
  action: string;
  description: string;
  area: string;
}

export interface Invitation {
  id: string;
  tenant_id: string;
  email?: string;
  username?: string;
  role: Role | (string & {});
  team_id?: string;
  invited_by: string;
  created_at: ISODateString;
  expires_at: ISODateString;
  accepted_at?: ISODateString;
  accepted_by?: string;
  revoked: boolean;
}

export interface InvitationPreview {
  invitation: Invitation;
  tenant: Tenant;
}

export interface ResetCode {
  code: string;
  expires_at: ISODateString;
}

export interface SsoConfig {
  enabled: boolean;
  name: string;
  issuer: string;
  client_id: string;
  /** The secret itself is never returned. */
  has_client_secret: boolean;
  redirect_url: string;
  scopes: string[] | null;
  allowed_domains: string[] | null;
  auto_create: boolean;
  username_claim: string;
  updated_at?: ISODateString;
}

export interface SsoConfigUpdate {
  enabled: boolean;
  name: string;
  issuer: string;
  client_id: string;
  /** Write-only: omit to keep the stored secret. */
  client_secret?: string;
  redirect_url: string;
  scopes: string[];
  allowed_domains: string[];
  auto_create: boolean;
  username_claim: string;
}

// ---- Endpoints ----

async function list<T>(p: Promise<T[] | null>): Promise<T[]> {
  return (await p) ?? [];
}

export function createSecurityApi(client: ApiClient) {
  return {
    providers: () => client.request<AuthProviders>(`${V1}/auth/providers`, { skipAuthRedirect: true }),
    /** URL that starts the SSO flow (full-page navigation). */
    ssoStartUrl: (redirect: string) => client.url(`${V1}/auth/sso/start`, { redirect }),

    login: (body: { username: string; password: string }) =>
      client.request<LoginResponse>(`${V1}/auth/login`, { method: "POST", body, skipAuthRedirect: true }),
    loginMfa: (body: { mfa_token: string; code: string }) =>
      client.request<AuthResponse>(`${V1}/auth/login/mfa`, { method: "POST", body, skipAuthRedirect: true }),

    mfa: {
      status: () => client.request<MfaState>(`${V1}/auth/mfa`),
      enroll: () => client.request<TotpEnrollment>(`${V1}/auth/mfa/totp/enroll`, { method: "POST" }),
      confirm: (code: string) =>
        client.request<RecoveryCodes>(`${V1}/auth/mfa/totp/confirm`, { method: "POST", body: { code } }),
      disable: (password: string) =>
        client.request<void>(`${V1}/auth/mfa/totp`, { method: "DELETE", body: { password } }),
      regenerateRecoveryCodes: (code: string) =>
        client.request<RecoveryCodes>(`${V1}/auth/mfa/recovery-codes`, { method: "POST", body: { code } }),
    },

    sessions: {
      list: () => list(client.request<Session[] | null>(`${V1}/auth/sessions`)),
      revoke: (id: string) => client.request<void>(`${V1}/auth/sessions/${seg(id)}`, { method: "DELETE" }),
      revokeOthers: () => client.request<{ revoked: number }>(`${V1}/auth/sessions/revoke-others`, { method: "POST" }),
    },

    tokens: {
      list: () => list(client.request<SecureToken[] | null>(`${V1}/auth/tokens`)),
      create: (body: CreateScopedTokenRequest) =>
        client.request<CreatedScopedToken>(`${V1}/auth/tokens`, { method: "POST", body }),
    },

    iamActions: () => list(client.request<IamAction[] | null>(`${V1}/iam/actions`, { skipAuthRedirect: true })),

    invitations: {
      preview: (code: string) =>
        client.request<InvitationPreview>(`${V1}/invitations/${seg(code)}`, { skipAuthRedirect: true }),
      accept: (code: string) => client.request<Invitation>(`${V1}/invitations/${seg(code)}/accept`, { method: "POST" }),
    },

    passwordReset: (body: { code: string; new_password: string }) =>
      client.request<void>(`${V1}/auth/password-reset`, { method: "POST", body, skipAuthRedirect: true }),

    admin: {
      resetCode: (username: string, ttlHours?: number) =>
        client.request<ResetCode>(`${V1}/admin/accounts/${seg(username)}/reset-code`, {
          method: "POST",
          body: ttlHours ? { ttl_hours: ttlHours } : {},
        }),
      sso: () => client.request<SsoConfig>(`${V1}/admin/sso`),
      saveSso: (body: SsoConfigUpdate) => client.request<SsoConfig>(`${V1}/admin/sso`, { method: "PUT", body }),
    },
  };
}

export type SecurityApi = ReturnType<typeof createSecurityApi>;

// ---- Hooks ----

export const securityKeys = {
  providers: ["auth", "providers"] as const,
  mfa: ["auth", "mfa"] as const,
  sessions: ["auth", "sessions"] as const,
  /** Shares the v0.3 key so other token views refresh too. */
  tokens: ["tokens"] as const,
  iamActions: ["iam", "actions"] as const,
  invitation: (code: string) => ["invitation", code] as const,
  sso: ["admin", "sso"] as const,
};

export function useSecurityApi(): SecurityApi {
  const api = useApi();
  return useMemo(() => createSecurityApi(api.client), [api]);
}

export function useAuthProviders() {
  const sec = useSecurityApi();
  return useQuery({ queryKey: securityKeys.providers, queryFn: sec.providers, staleTime: 60_000 });
}

export function useMfaStatus() {
  const sec = useSecurityApi();
  return useQuery({ queryKey: securityKeys.mfa, queryFn: sec.mfa.status });
}

export function useSessions() {
  const sec = useSecurityApi();
  return useQuery({ queryKey: securityKeys.sessions, queryFn: sec.sessions.list });
}

export function useSecureTokens() {
  const sec = useSecurityApi();
  return useQuery({ queryKey: securityKeys.tokens, queryFn: sec.tokens.list });
}

export function useIamActions(enabled = true) {
  const sec = useSecurityApi();
  return useQuery({ queryKey: securityKeys.iamActions, queryFn: sec.iamActions, staleTime: 10 * 60_000, enabled });
}

export function useInvitationPreview(code: string) {
  const sec = useSecurityApi();
  return useQuery({ queryKey: securityKeys.invitation(code), queryFn: () => sec.invitations.preview(code) });
}

export function useSsoConfig(enabled = true) {
  const sec = useSecurityApi();
  return useQuery({ queryKey: securityKeys.sso, queryFn: sec.admin.sso, enabled });
}
