/** Site-wide constants shared by marketing pages, layouts and the docs shell. */

export const SITE_NAME = "W0rkTree";

/** Version the documentation describes. Bump together with the changelog. */
export const CURRENT_VERSION = "v0.8.0";
/** Content dated before this (ISO date) predates CURRENT_VERSION's design. */
export const CURRENT_VERSION_DATE = "2026-01-01";

export const REPO_URL = "https://github.com/seanfilimon/worktree";
export const ISSUES_URL = `${REPO_URL}/issues`;
export const CONTRIBUTING_URL = `${REPO_URL}/blob/main/CONTRIBUTING`;
export const LICENSE_URL = `${REPO_URL}/blob/main/LICENSE`;
export const DISCORD_URL = "https://discord.gg/worktree";

export const LICENSE_NAME = "W0rkTree Public License 1.0";

/** Default address of the W0rkTree server (gRPC + REST + admin on one port). */
export const DEFAULT_SERVER_URL = "http://localhost:7420";
