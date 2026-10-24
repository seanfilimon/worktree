const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 3600],
  ["month", 30 * 24 * 3600],
  ["week", 7 * 24 * 3600],
  ["day", 24 * 3600],
  ["hour", 3600],
  ["minute", 60],
];

const rtf = new Intl.RelativeTimeFormat("en", { numeric: "auto" });

/** "3 minutes ago", "yesterday", "just now". Invalid dates yield "—". */
export function relativeTime(iso: string | undefined, now: number = Date.now()): string {
  if (!iso) return "—";
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "—";
  const diff = Math.round((t - now) / 1000);
  const abs = Math.abs(diff);
  if (abs < 45) return "just now";
  for (const [unit, secs] of UNITS) {
    if (abs >= secs) return rtf.format(Math.round(diff / secs), unit);
  }
  return rtf.format(Math.round(diff / 60), "minute");
}

const dtf = new Intl.DateTimeFormat("en", { dateStyle: "medium", timeStyle: "short" });

export function absoluteTime(iso: string | undefined): string {
  if (!iso) return "";
  const t = Date.parse(iso);
  return Number.isNaN(t) ? "" : dtf.format(t);
}

/** Binary (IEC) byte sizes: "0 B", "1.5 KiB", "2.0 GiB". */
export function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n < 0) return "—";
  const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return i === 0 ? `${v} B` : `${v.toFixed(1)} ${units[i]}`;
}

const nf = new Intl.NumberFormat("en");
export function formatNumber(n: number): string {
  return Number.isFinite(n) ? nf.format(n) : "—";
}

/** "3d 4h", "12m", "45s". */
export function formatDuration(totalSeconds: number): string {
  if (!Number.isFinite(totalSeconds) || totalSeconds < 0) return "—";
  const s = Math.floor(totalSeconds);
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m`;
  return `${s}s`;
}

/** First 8 characters of a snapshot id or hash. */
export function shortId(id: string | undefined): string {
  return id ? id.replace(/-/g, "").slice(0, 8) : "";
}

/** First line of a (possibly multi-line) message. */
export function firstLine(message: string): string {
  const line = message.split(/\r?\n/, 1)[0] ?? "";
  return line.trim();
}
