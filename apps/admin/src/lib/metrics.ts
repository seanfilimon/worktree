/**
 * Prometheus text exposition format (v0.0.4) parser plus helpers that turn
 * the server's cumulative HTTP counters (see services/server-go
 * internal/httpapi/metrics.go) into request-rate and latency points.
 */

export type MetricType = "counter" | "gauge" | "histogram" | "summary" | "untyped";

export interface MetricSample {
  /** Full sample name, e.g. `worktree_http_request_duration_seconds_bucket`. */
  name: string;
  labels: Record<string, string>;
  value: number;
  /** Optional sample timestamp (ms since epoch). */
  timestamp?: number;
}

export interface MetricFamily {
  name: string;
  type: MetricType;
  help: string;
  samples: MetricSample[];
}

/** Metric families keyed by family name, in exposition order. */
export type MetricFamilies = Map<string, MetricFamily>;

const TYPES = new Set<MetricType>(["counter", "gauge", "histogram", "summary", "untyped"]);
const SUFFIXES: Record<string, readonly string[]> = {
  histogram: ["_bucket", "_sum", "_count"],
  summary: ["_sum", "_count"],
};

function parseValue(raw: string): number {
  switch (raw) {
    case "+Inf":
    case "Inf":
      return Infinity;
    case "-Inf":
      return -Infinity;
    case "NaN":
      return NaN;
    default:
      return raw === "" ? NaN : Number(raw);
  }
}

function unescape(s: string, inHelp: boolean): string {
  return s.replace(/\\(.)/g, (m, c: string) => {
    if (c === "n") return "\n";
    if (c === "\\") return "\\";
    if (c === '"' && !inHelp) return '"';
    return m;
  });
}

/** Parses `{a="1",b="x\"y"}` starting at `start` (the `{`). Returns labels and the index after `}`. */
function parseLabels(line: string, start: number): { labels: Record<string, string>; end: number } | null {
  const labels: Record<string, string> = {};
  let i = start + 1;
  for (;;) {
    while (line[i] === " " || line[i] === ",") i++;
    if (line[i] === "}") return { labels, end: i + 1 };
    const eq = line.indexOf("=", i);
    if (eq < 0 || line[eq + 1] !== '"') return null;
    const key = line.slice(i, eq).trim();
    let j = eq + 2;
    let raw = "";
    while (j < line.length && line[j] !== '"') {
      if (line[j] === "\\" && j + 1 < line.length) {
        raw += line.slice(j, j + 2);
        j += 2;
      } else {
        raw += line[j];
        j++;
      }
    }
    if (j >= line.length) return null;
    labels[key] = unescape(raw, false);
    i = j + 1;
  }
}

function parseSample(line: string): MetricSample | null {
  const nameMatch = /^[a-zA-Z_:][a-zA-Z0-9_:]*/.exec(line);
  if (!nameMatch) return null;
  const name = nameMatch[0];
  let rest = line.slice(name.length);
  let labels: Record<string, string> = {};
  if (rest.startsWith("{")) {
    const parsed = parseLabels(line, name.length);
    if (!parsed) return null;
    labels = parsed.labels;
    rest = line.slice(parsed.end);
  }
  const parts = rest.trim().split(/\s+/);
  if (parts.length === 0 || parts[0] === "") return null;
  const value = parseValue(parts[0]!);
  if (Number.isNaN(value) && parts[0] !== "NaN") return null;
  const sample: MetricSample = { name, labels, value };
  if (parts[1] !== undefined) {
    const ts = Number(parts[1]);
    if (Number.isFinite(ts)) sample.timestamp = ts;
  }
  return sample;
}

/**
 * Parses Prometheus text. Malformed lines are skipped. Histogram and summary
 * sub-samples (`_bucket`, `_sum`, `_count`) are grouped under their family.
 */
export function parsePrometheus(text: string): MetricFamilies {
  const families: MetricFamilies = new Map();
  const family = (name: string): MetricFamily => {
    let f = families.get(name);
    if (!f) {
      f = { name, type: "untyped", help: "", samples: [] };
      families.set(name, f);
    }
    return f;
  };

  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (line === "") continue;
    if (line.startsWith("#")) {
      const m = /^#\s*(HELP|TYPE)\s+([a-zA-Z_:][a-zA-Z0-9_:]*)\s?(.*)$/.exec(line);
      if (!m) continue;
      const [, kind, name, rest = ""] = m;
      const f = family(name!);
      if (kind === "HELP") f.help = unescape(rest, true);
      else if (TYPES.has(rest.trim() as MetricType)) f.type = rest.trim() as MetricType;
      continue;
    }
    const sample = parseSample(line);
    if (!sample) continue;
    family(familyName(families, sample.name)).samples.push(sample);
  }
  return families;
}

function familyName(families: MetricFamilies, sample: string): string {
  if (families.has(sample)) {
    const f = families.get(sample)!;
    // A histogram's own name never carries samples directly.
    if (!SUFFIXES[f.type]) return sample;
  }
  for (const [type, suffixes] of Object.entries(SUFFIXES)) {
    for (const suffix of suffixes) {
      if (!sample.endsWith(suffix)) continue;
      const base = sample.slice(0, -suffix.length);
      if (families.get(base)?.type === type) return base;
    }
  }
  return sample;
}

/** Samples named exactly `name` (searching its family), optionally filtered by labels. */
export function samplesOf(families: MetricFamilies, name: string, match: Record<string, string> = {}): MetricSample[] {
  const out: MetricSample[] = [];
  for (const f of families.values()) {
    for (const s of f.samples) {
      if (s.name === name && Object.entries(match).every(([k, v]) => s.labels[k] === v)) out.push(s);
    }
  }
  return out;
}

/** First value of a (usually unlabelled) gauge or counter, or undefined. */
export function metricValue(families: MetricFamilies, name: string): number | undefined {
  return samplesOf(families, name)[0]?.value;
}

/** Cumulative request counters summed over every route. */
export interface RequestTotals {
  requests: number;
  /** Responses with a 5xx status. */
  errors: number;
  durationSum: number;
  durationCount: number;
  /** Cumulative bucket counts keyed by upper bound (seconds), ascending. */
  buckets: [le: number, count: number][];
}

const REQUESTS = "worktree_http_requests_total";
const DURATION = "worktree_http_request_duration_seconds";

export function requestTotals(families: MetricFamilies): RequestTotals {
  let requests = 0;
  let errors = 0;
  for (const s of samplesOf(families, REQUESTS)) {
    requests += s.value;
    if ((s.labels.code ?? "").startsWith("5")) errors += s.value;
  }
  const sum = (name: string) => samplesOf(families, name).reduce((acc, s) => acc + s.value, 0);
  const byLe = new Map<number, number>();
  for (const s of samplesOf(families, `${DURATION}_bucket`)) {
    const le = parseValue(s.labels.le ?? "");
    if (!Number.isNaN(le)) byLe.set(le, (byLe.get(le) ?? 0) + s.value);
  }
  return {
    requests,
    errors,
    durationSum: sum(`${DURATION}_sum`),
    durationCount: sum(`${DURATION}_count`),
    buckets: [...byLe.entries()].sort((a, b) => a[0] - b[0]),
  };
}

/**
 * Estimates quantile `q` (0..1) from cumulative histogram buckets with linear
 * interpolation, like PromQL's histogram_quantile. Returns undefined when the
 * histogram is empty.
 */
export function histogramQuantile(q: number, buckets: [le: number, count: number][]): number | undefined {
  if (buckets.length === 0) return undefined;
  const total = buckets.at(-1)![1];
  if (!(total > 0)) return undefined;
  const rank = q * total;
  let prevLe = 0;
  let prevCount = 0;
  for (const [le, count] of buckets) {
    if (count >= rank) {
      // The +Inf bucket has no upper bound: report the highest finite one.
      if (!Number.isFinite(le)) return prevLe;
      if (count === prevCount) return le;
      return prevLe + ((le - prevLe) * (rank - prevCount)) / (count - prevCount);
    }
    prevLe = le;
    prevCount = count;
  }
  return prevLe;
}

/** One request-rate / latency point between two scrapes. */
export interface RequestRatePoint {
  /** End of the interval (ms since epoch). */
  t: number;
  /** Requests per second. */
  rps: number;
  /** 5xx responses per second. */
  errorsPerSec: number;
  /** Mean latency over the interval, ms (undefined without traffic). */
  avgMs: number | undefined;
  /** Estimated 95th percentile latency, ms (undefined without traffic). */
  p95Ms: number | undefined;
}

/**
 * Rates between two scrapes taken at `prevT` and `nextT` (ms). A counter that
 * went backwards means the server restarted: `next` is then the whole delta.
 */
export function requestRate(prev: RequestTotals, prevT: number, next: RequestTotals, nextT: number): RequestRatePoint {
  const seconds = Math.max((nextT - prevT) / 1000, 1e-3);
  const reset = next.requests < prev.requests || next.durationCount < prev.durationCount;
  const d = (p: number, n: number) => (reset ? n : Math.max(0, n - p));
  const count = d(prev.durationCount, next.durationCount);
  const sum = d(prev.durationSum, next.durationSum);
  const prevBuckets = new Map(prev.buckets);
  const buckets = next.buckets.map(([le, c]): [number, number] => [le, d(prevBuckets.get(le) ?? 0, c)]);
  const p95 = count > 0 ? histogramQuantile(0.95, buckets) : undefined;
  return {
    t: nextT,
    rps: d(prev.requests, next.requests) / seconds,
    errorsPerSec: d(prev.errors, next.errors) / seconds,
    avgMs: count > 0 ? (sum / count) * 1000 : undefined,
    p95Ms: p95 === undefined ? undefined : p95 * 1000,
  };
}
