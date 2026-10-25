import type {
  AuthProviders,
  IamAction,
  InvitationPreview,
  MfaState,
  SecureToken,
  Session,
  SsoConfig,
} from "@/api/security";
import * as fx from "./fixtures";

const recent = (minutesAgo: number) => new Date(Date.now() - minutesAgo * 60_000).toISOString();
const inDays = (days: number) => new Date(Date.now() + days * 86_400_000).toISOString();

export const providers: AuthProviders = { password: true, registration: true, sso: { enabled: false, name: "" } };
export const providersSso: AuthProviders = { password: true, registration: true, sso: { enabled: true, name: "Okta" } };

export const mfaOff: MfaState = { account_id: "acc-1", totp_enabled: false, recovery_codes_left: 0 };
export const mfaOn: MfaState = {
  account_id: "acc-1",
  totp_enabled: true,
  enabled_at: recent(60 * 24),
  recovery_codes_left: 8,
};

export const enrollment = {
  secret: "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP",
  otpauth_url: "otpauth://totp/W0rkTree:alice?secret=JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP&issuer=W0rkTree",
};

export const recoveryCodes = Array.from({ length: 10 }, (_, i) => `ab${i}d-12${i}4`);

export const sessions: Session[] = [
  {
    id: "ses-1",
    account_id: "acc-1",
    name: "session",
    kind: "session",
    created_at: recent(120),
    last_used_at: recent(1),
    last_ip: "10.0.0.5",
    user_agent:
      "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0 Safari/537.36",
    mfa: true,
    current: true,
  },
  {
    id: "ses-2",
    account_id: "acc-1",
    name: "session",
    kind: "session",
    created_at: recent(60 * 48),
    last_used_at: recent(60 * 5),
    last_ip: "203.0.113.9",
    user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5) Gecko/20100101 Firefox/131.0",
    current: false,
  },
  {
    id: "ses-3",
    account_id: "acc-1",
    name: "session",
    kind: "session",
    created_at: recent(60 * 72),
    last_ip: "198.51.100.2",
    user_agent: "wt/0.4.0 (windows)",
    current: false,
  },
];

export const scopedToken: SecureToken = {
  id: "tk-s",
  account_id: "acc-1",
  name: "ci-read",
  kind: "api",
  created_at: recent(30),
  scopes: ["tree:read", "mr:*"],
};

export const iamActions: IamAction[] = [
  { action: "tree:read", area: "tree", description: "Read trees" },
  { action: "tree:write", area: "tree", description: "Push, stage and tag" },
  { action: "mr:create", area: "mr", description: "Open merge requests" },
  { action: "mr:merge", area: "mr", description: "Merge merge requests" },
];

export const invitation: InvitationPreview = {
  invitation: {
    id: "inv-1",
    tenant_id: fx.tenant.id,
    email: "alice@example.com",
    role: "developer",
    invited_by: "bob",
    created_at: recent(60),
    expires_at: inDays(6),
    revoked: false,
  },
  tenant: fx.tenant,
};

export const ssoConfig: SsoConfig = {
  enabled: true,
  name: "Okta",
  issuer: "https://acme.okta.com",
  client_id: "wt-admin",
  has_client_secret: true,
  redirect_url: "https://wt.example.com/api/v1/auth/sso/callback",
  scopes: ["openid", "email", "profile"],
  allowed_domains: ["example.com"],
  auto_create: false,
  username_claim: "preferred_username",
  updated_at: recent(60 * 24 * 3),
};
