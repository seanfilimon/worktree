/** "Firefox on Windows", "wt CLI 0.4.0", … from a User-Agent header. */
export function describeUserAgent(ua: string | undefined): string {
  if (!ua) return "Unknown client";
  const wt = /\b(wt|worktree-cli|worktree-sdk)\/([\w.-]+)/i.exec(ua);
  if (wt) return `wt CLI ${wt[2]}`;
  const browser = /Edg\//.test(ua)
    ? "Edge"
    : /OPR\/|Opera/.test(ua)
      ? "Opera"
      : /Firefox\//.test(ua)
        ? "Firefox"
        : /Chrome\/|CriOS\//.test(ua)
          ? "Chrome"
          : /Safari\//.test(ua)
            ? "Safari"
            : /curl\//.test(ua)
              ? "curl"
              : null;
  const os = /Windows/.test(ua)
    ? "Windows"
    : /iPhone|iPad|iOS/.test(ua)
      ? "iOS"
      : /Mac OS X|Macintosh/.test(ua)
        ? "macOS"
        : /Android/.test(ua)
          ? "Android"
          : /Linux/.test(ua)
            ? "Linux"
            : null;
  if (browser && os) return `${browser} on ${os}`;
  if (browser) return browser;
  return ua.length > 48 ? `${ua.slice(0, 47)}…` : ua;
}
