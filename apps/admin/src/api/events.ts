import { buildQuery } from "./client";
import { EVENT_TYPES, type ServerEvent } from "./types";

/** The subset of EventSource this client relies on (eases testing). */
export interface EventSourceLike {
  readonly readyState: number;
  onopen: ((this: EventSource, ev: Event) => unknown) | null;
  onerror: ((this: EventSource, ev: Event) => unknown) | null;
  onmessage: ((this: EventSource, ev: MessageEvent) => unknown) | null;
  addEventListener(type: string, listener: (ev: MessageEvent) => void): void;
  close(): void;
}

export type EventSourceFactory = (url: string) => EventSourceLike;

export type StreamStatus = "connecting" | "open" | "reconnecting" | "closed";

export interface EventFilter {
  /** Tenant slug. */
  tenant?: string;
  /** Tree name (requires tenant). */
  tree?: string;
}

export interface EventStreamOptions {
  baseUrl?: string;
  filter?: EventFilter;
  /** Read on every (re)connect so a refreshed token is picked up. */
  getToken: () => string | null;
  onEvent: (event: ServerEvent) => void;
  onStatus?: (status: StreamStatus) => void;
  factory?: EventSourceFactory;
  /** First reconnect delay in ms (doubles per failed attempt). */
  minDelayMs?: number;
  maxDelayMs?: number;
  /** Returns [0, 1); used for jitter. */
  random?: () => number;
}

const defaultFactory: EventSourceFactory = (url) => new EventSource(url);

/** Exponential backoff with ±20% jitter, capped at `max`. */
export function backoffDelay(attempt: number, min: number, max: number, random: () => number): number {
  const base = Math.min(max, min * 2 ** Math.max(0, attempt));
  const jitter = base * 0.2 * (random() * 2 - 1);
  return Math.max(0, Math.min(max, Math.round(base + jitter)));
}

function parseEvent(type: string, data: unknown): ServerEvent | null {
  if (typeof data !== "string") return null;
  try {
    const parsed = JSON.parse(data) as Partial<ServerEvent> | null;
    if (!parsed || typeof parsed !== "object") return null;
    return {
      ...parsed,
      type: parsed.type ?? type,
      tenant_id: parsed.tenant_id ?? "",
      at: parsed.at ?? new Date().toISOString(),
    };
  } catch {
    return null;
  }
}

/**
 * Client for `GET /api/v1/events` (text/event-stream).
 *
 * EventSource cannot send headers, so the bearer token is passed as
 * `?access_token=`. Instead of relying on the browser's built-in retry (which
 * stops for good on HTTP errors and keeps a stale token), every error closes
 * the source and reconnects with exponential backoff; a successful open
 * resets the backoff.
 */
export class EventStream {
  private source: EventSourceLike | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private attempt = 0;
  private stopped = true;
  private readonly opts: Required<Omit<EventStreamOptions, "filter" | "onStatus">> &
    Pick<EventStreamOptions, "filter" | "onStatus">;

  constructor(opts: EventStreamOptions) {
    this.opts = {
      baseUrl: "",
      factory: defaultFactory,
      minDelayMs: 1000,
      maxDelayMs: 30_000,
      random: Math.random,
      ...opts,
    };
  }

  /** URL for the next connection attempt. */
  url(): string {
    const { baseUrl, filter, getToken } = this.opts;
    return `${baseUrl}/api/v1/events${buildQuery({
      tenant: filter?.tenant,
      tree: filter?.tenant ? filter.tree : undefined,
      access_token: getToken(),
    })}`;
  }

  start(): void {
    if (!this.stopped) return;
    this.stopped = false;
    this.attempt = 0;
    this.connect("connecting");
  }

  stop(): void {
    this.stopped = true;
    this.clearTimer();
    this.closeSource();
    this.opts.onStatus?.("closed");
  }

  private connect(status: StreamStatus): void {
    this.opts.onStatus?.(status);
    const source = this.opts.factory(this.url());
    this.source = source;

    const deliver = (type: string) => (ev: MessageEvent) => {
      if (this.source !== source) return;
      const event = parseEvent(type, ev.data);
      if (event) this.opts.onEvent(event);
    };
    for (const type of EVENT_TYPES) source.addEventListener(type, deliver(type));
    source.onmessage = deliver("message");

    source.onopen = () => {
      if (this.source !== source) return;
      this.attempt = 0;
      this.opts.onStatus?.("open");
    };
    source.onerror = () => {
      if (this.source !== source || this.stopped) return;
      this.closeSource();
      this.scheduleReconnect();
    };
  }

  private scheduleReconnect(): void {
    this.clearTimer();
    const { minDelayMs, maxDelayMs, random } = this.opts;
    const delay = backoffDelay(this.attempt, minDelayMs, maxDelayMs, random);
    this.attempt += 1;
    this.opts.onStatus?.("reconnecting");
    this.timer = setTimeout(() => {
      this.timer = null;
      if (!this.stopped) this.connect("reconnecting");
    }, delay);
  }

  private closeSource(): void {
    const s = this.source;
    this.source = null;
    if (s) {
      s.onopen = null;
      s.onerror = null;
      s.onmessage = null;
      s.close();
    }
  }

  private clearTimer(): void {
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
  }
}
