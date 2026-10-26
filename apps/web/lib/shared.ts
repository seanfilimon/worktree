import { CURRENT_VERSION, REPO_URL, SITE_NAME } from "./site";

export const appName = SITE_NAME;
export const appDescription =
  "Version control with nested trees, automatic change tracking, team-visible work in progress and a self-hosted multi-tenant server.";
/** The documented release without its leading "v" (lib/site.ts holds "v0.8.0"). */
export const version: string = CURRENT_VERSION.replace(/^v/, "");
/** W0rkTree's creator, credited in the site and docs footers, the hero and every docs page. */
export const creator = "Sean Filimon";

/** The public repository: header button, footer, "Edit this page" and issue links. */
export const repositoryUrl: string = REPO_URL;
export const repositoryBranch = "main";

/** A link to a file in the repository. */
export function sourceFileUrl(path: string): string {
  return `${repositoryUrl}/blob/${repositoryBranch}/${path}`;
}

/** A prefilled "new issue" link. */
export function newIssueUrl(title: string, body: string): string {
  return `${repositoryUrl}/issues/new?${new URLSearchParams({ title, body, labels: "documentation" })}`;
}

/** Canonical origin for metadata; on Railway, the service's public domain. */
export const siteUrl = (
  process.env.SITE_URL ??
  (process.env.RAILWAY_PUBLIC_DOMAIN
    ? `https://${process.env.RAILWAY_PUBLIC_DOMAIN}`
    : "http://localhost:3000")
).replace(/\/$/, "");
