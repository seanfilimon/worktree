/** Strips spaces users copy from authenticator apps ("123 456"). */
export function normalizeTotp(code: string): string {
  return code.replace(/\s+/g, "");
}

export const TOTP_RE = /^\d{6}$/;

/** Recovery codes are `xxxx-xxxx`; accept any case and a missing dash. */
export function normalizeRecoveryCode(code: string): string {
  const c = code.trim().toLowerCase().replace(/\s+/g, "");
  return /^[a-z0-9]{8}$/.test(c) ? `${c.slice(0, 4)}-${c.slice(4)}` : c;
}

export const RECOVERY_RE = /^[a-z0-9]{4}-[a-z0-9]{4}$/;
