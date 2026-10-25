/**
 * Client-side mirrors of the server's naming rules
 * (services/server-go/internal/httpapi/helpers.go, internal/iam/validate.go).
 * The server stays the authority; these only give early feedback.
 */

/** Tenant slug: 1-63 of a-z, 0-9 and '-', not starting or ending with '-'. */
export const SLUG_RE = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/;
export const SLUG_HINT = "1–63 lowercase letters, digits or dashes; no leading or trailing dash.";

/** Tree name: starts alphanumeric, then up to 99 of A-Z, a-z, 0-9, '.', '_', '-'. */
export const TREE_NAME_RE = /^[A-Za-z0-9][A-Za-z0-9._-]{0,99}$/;
export const TREE_NAME_HINT = "Letters, digits, '.', '_' or '-', starting with a letter or digit.";

/** Account username: 2-39 of a-z, 0-9 and '-', starting alphanumeric. */
export const USERNAME_RE = /^[a-z0-9][a-z0-9-]{1,38}$/;
export const USERNAME_HINT = "2–39 lowercase letters, digits or dashes, starting with a letter or digit.";

export const MIN_PASSWORD_LENGTH = 8;

/** Derives a tenant slug from a display name: "Acme Corp!" → "acme-corp". */
export function slugify(name: string): string {
  return name
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+/, "")
    .slice(0, 63)
    .replace(/-+$/, "");
}
