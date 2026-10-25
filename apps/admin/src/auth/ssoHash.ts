/**
 * The SSO callback (§12.3 [iam-sso]) redirects back to the panel with the
 * result in the URL fragment: `<redirect>#sso_token=<token>` on success,
 * `/admin/login#sso_error=<message>` on failure. The fragment never reaches
 * the server; it is read once on load and then stripped from the address bar
 * so the token is not left in history.
 */
export interface SsoHashResult {
  token: string | null;
  error: string | null;
}

export function readSsoHash(hash: string = globalThis.location?.hash ?? ""): SsoHashResult {
  if (!hash.startsWith("#") || (!hash.includes("sso_token=") && !hash.includes("sso_error="))) {
    return { token: null, error: null };
  }
  const params = new URLSearchParams(hash.slice(1));
  const token = params.get("sso_token");
  const error = params.get("sso_error");
  return {
    token: token || null,
    error: token ? null : error || (params.has("sso_error") ? "Single sign-on failed." : null),
  };
}

/** Removes the SSO fragment from the current URL without navigating. */
export function clearSsoHash(): void {
  try {
    const { pathname, search, hash } = window.location;
    if (!readSsoHash(hash).token && !readSsoHash(hash).error) return;
    window.history.replaceState(window.history.state, "", `${pathname}${search}`);
  } catch {
    // History may be unavailable (sandboxed frames); the token is already stored.
  }
}

/** Builds the same-origin path the server redirects to after SSO. */
export function appPath(path: string): string {
  const base = (import.meta.env.BASE_URL ?? "/").replace(/\/+$/, "");
  return `${base}${path.startsWith("/") ? path : `/${path}`}`;
}

/** Absolute URL of an app route, e.g. for links shared out of band. */
export function appUrl(path: string): string {
  return `${window.location.origin}${appPath(path)}`;
}
