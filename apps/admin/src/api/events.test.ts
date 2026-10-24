import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FakeEventSource } from "@/test/fakeEventSource";
import { backoffDelay, EventStream, type StreamStatus } from "./events";
import type { ServerEvent } from "./types";

function setup(token = "tok") {
  const events: ServerEvent[] = [];
  const statuses: StreamStatus[] = [];
  let current = token;
  const stream = new EventStream({
    filter: { tenant: "acme", tree: "web" },
    getToken: () => current,
    onEvent: (e) => events.push(e),
    onStatus: (s) => statuses.push(s),
    factory: (url) => new FakeEventSource(url),
    minDelayMs: 1000,
    maxDelayMs: 8000,
    random: () => 0.5, // no jitter
  });
  return { stream, events, statuses, setToken: (t: string) => (current = t) };
}

describe("EventStream", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("connects with filters and the access token in the query", () => {
    const { stream } = setup("secret token");
    stream.start();
    const url = new URL(FakeEventSource.latest().url, "http://x");
    expect(url.pathname).toBe("/api/v1/events");
    expect(url.searchParams.get("tenant")).toBe("acme");
    expect(url.searchParams.get("tree")).toBe("web");
    expect(url.searchParams.get("access_token")).toBe("secret token");
  });

  it("delivers named events with parsed JSON", () => {
    const { stream, events, statuses } = setup();
    stream.start();
    const es = FakeEventSource.latest();
    es.open();
    es.emit("branch.pushed", { type: "branch.pushed", tenant_id: "t1", actor: "alice", payload: { branch: "main" }, at: "2026-01-01T00:00:00Z" });
    expect(statuses).toEqual(["connecting", "open"]);
    expect(events).toHaveLength(1);
    expect(events[0]).toMatchObject({ type: "branch.pushed", actor: "alice", payload: { branch: "main" } });
  });

  it("ignores malformed event data", () => {
    const { stream, events } = setup();
    stream.start();
    FakeEventSource.latest().emit("mr.opened", "{not json");
    expect(events).toHaveLength(0);
  });

  it("fills in the type from the SSE event name when missing", () => {
    const { stream, events } = setup();
    stream.start();
    FakeEventSource.latest().emit("tag.pushed", { tenant_id: "t1", at: "2026-01-01T00:00:00Z" });
    expect(events[0]?.type).toBe("tag.pushed");
  });

  it("reconnects with exponential backoff after errors", () => {
    const { stream, statuses } = setup();
    stream.start();
    const first = FakeEventSource.latest();
    first.fail();
    expect(first.closed).toBe(true);
    expect(statuses.at(-1)).toBe("reconnecting");

    vi.advanceTimersByTime(999);
    expect(FakeEventSource.instances).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(FakeEventSource.instances).toHaveLength(2);

    FakeEventSource.latest().fail();
    vi.advanceTimersByTime(1999);
    expect(FakeEventSource.instances).toHaveLength(2);
    vi.advanceTimersByTime(1);
    expect(FakeEventSource.instances).toHaveLength(3);
  });

  it("resets the backoff once a connection opens", () => {
    const { stream } = setup();
    stream.start();
    FakeEventSource.latest().fail();
    vi.advanceTimersByTime(1000);
    FakeEventSource.latest().fail();
    vi.advanceTimersByTime(2000);
    FakeEventSource.latest().open();
    FakeEventSource.latest().fail();
    vi.advanceTimersByTime(1000);
    expect(FakeEventSource.instances).toHaveLength(4);
  });

  it("uses the current token on reconnect", () => {
    const { stream, setToken } = setup("old");
    stream.start();
    setToken("new");
    FakeEventSource.latest().fail();
    vi.advanceTimersByTime(1000);
    expect(new URL(FakeEventSource.latest().url, "http://x").searchParams.get("access_token")).toBe("new");
  });

  it("stops reconnecting after stop()", () => {
    const { stream, statuses } = setup();
    stream.start();
    FakeEventSource.latest().fail();
    stream.stop();
    vi.advanceTimersByTime(60_000);
    expect(FakeEventSource.instances).toHaveLength(1);
    expect(statuses.at(-1)).toBe("closed");
  });

  it("ignores events from a source that was replaced", () => {
    const { stream, events } = setup();
    stream.start();
    const stale = FakeEventSource.latest();
    stale.fail();
    vi.advanceTimersByTime(1000);
    stale.emit("branch.pushed", { type: "branch.pushed", tenant_id: "t", at: "2026-01-01T00:00:00Z" });
    expect(events).toHaveLength(0);
  });
});

describe("backoffDelay", () => {
  it("doubles per attempt and caps at the maximum", () => {
    const mid = () => 0.5;
    expect([0, 1, 2, 3, 4, 10].map((a) => backoffDelay(a, 1000, 8000, mid))).toEqual([1000, 2000, 4000, 8000, 8000, 8000]);
  });

  it("applies at most ±20% jitter", () => {
    expect(backoffDelay(1, 1000, 30_000, () => 0)).toBe(1600);
    expect(backoffDelay(1, 1000, 30_000, () => 0.999999)).toBe(2400);
  });
});
