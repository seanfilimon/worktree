/**
 * v0.8 device authorization (docs/ENGINEERING_PLAN.md §16.6): the signed-in
 * user reviews, approves or denies a `wt login --device` request by its
 * user code. JSON shapes mirror services/server-go/internal/iam/device.go.
 */
import type { ApiClient } from "./client";
import type { ISODateString } from "./types";

const V1 = "/api/v1/auth/device";

export type DeviceStatus = "pending" | "approved" | "denied";

/** GET /auth/device/{user_code}. */
export interface DeviceRequest {
  user_code: string;
  client_id?: string;
  client_name: string;
  /** Requested scopes; empty = full account access. */
  scopes: string[] | null;
  status: DeviceStatus;
  remote_ip?: string;
  user_agent?: string;
  created_at: ISODateString;
  expires_at: ISODateString;
  /** Approving needs a TOTP or recovery code. */
  mfa_required: boolean;
}

/** User codes use 20 consonants (no vowels, no look-alikes). */
const ALPHABET = "BCDFGHJKLMNPQRSTVWXZ";

/**
 * Canonical "XXXX-XXXX" form of a user code typed in any case, with or
 * without the hyphen and spaces; null when it is not a valid code.
 */
export function normalizeUserCode(input: string): string | null {
  const chars = input.toUpperCase().replace(/[\s-]/g, "");
  if (chars.length !== 8 || [...chars].some((c) => !ALPHABET.includes(c))) return null;
  return `${chars.slice(0, 4)}-${chars.slice(4)}`;
}

/** Live formatting of the code input: upper case, hyphen after 4 characters. */
export function formatUserCodeInput(input: string): string {
  const chars = input
    .toUpperCase()
    .replace(/[^A-Z]/g, "")
    .slice(0, 8);
  return chars.length > 4 ? `${chars.slice(0, 4)}-${chars.slice(4)}` : chars;
}

export function createDeviceApi(client: ApiClient) {
  const path = (code: string) => `${V1}/${encodeURIComponent(code)}`;
  return {
    get: (code: string) => client.request<DeviceRequest>(path(code)),
    approve: (code: string, mfaCode?: string) =>
      client.request<DeviceRequest>(`${path(code)}/approve`, {
        method: "POST",
        body: mfaCode ? { code: mfaCode } : {},
      }),
    deny: (code: string) => client.request<DeviceRequest>(`${path(code)}/deny`, { method: "POST" }),
  };
}

export const deviceKeys = {
  request: (code: string) => ["device", code] as const,
};
