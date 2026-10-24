import type { EventSourceLike } from "@/api/events";

/** Minimal controllable EventSource for tests. */
export class FakeEventSource implements EventSourceLike {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSED = 2;
  static instances: FakeEventSource[] = [];

  static reset(): void {
    FakeEventSource.instances = [];
  }

  static latest(): FakeEventSource {
    const s = FakeEventSource.instances.at(-1);
    if (!s) throw new Error("no EventSource was created");
    return s;
  }

  readonly url: string;
  readyState = FakeEventSource.CONNECTING;
  closed = false;
  onopen: EventSourceLike["onopen"] = null;
  onerror: EventSourceLike["onerror"] = null;
  onmessage: EventSourceLike["onmessage"] = null;
  private listeners = new Map<string, ((ev: MessageEvent) => void)[]>();

  constructor(url: string) {
    this.url = url;
    FakeEventSource.instances.push(this);
  }

  addEventListener(type: string, listener: (ev: MessageEvent) => void): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }

  close(): void {
    this.closed = true;
    this.readyState = FakeEventSource.CLOSED;
  }

  // ---- test controls ----

  open(): void {
    this.readyState = FakeEventSource.OPEN;
    this.onopen?.call(this as unknown as EventSource, new Event("open"));
  }

  fail(): void {
    this.readyState = FakeEventSource.CLOSED;
    this.onerror?.call(this as unknown as EventSource, new Event("error"));
  }

  /** Dispatches a named SSE event (`event: <type>`) with a JSON payload. */
  emit(type: string, data: unknown): void {
    const ev = new MessageEvent(type, { data: typeof data === "string" ? data : JSON.stringify(data) });
    if (type === "message") this.onmessage?.call(this as unknown as EventSource, ev);
    for (const l of this.listeners.get(type) ?? []) l(ev);
  }
}
