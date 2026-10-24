import { vi } from "vitest";

export interface MockRequest {
  method: string;
  url: URL;
  headers: Headers;
  body: unknown;
}

export type MockHandler = (req: MockRequest) => unknown;

/** A route value: a handler, a ready Response, or a JSON body (200). */
export type MockRoute = MockHandler | Response | object | string | null;

export function json(body: unknown, status = 200): Response {
  return new Response(status === 204 ? null : JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

export function apiError(status: number, code: string, message: string, extra: Record<string, unknown> = {}): Response {
  return json({ error: { code, message, ...extra } }, status);
}

export function noContent(): Response {
  return new Response(null, { status: 204 });
}

/**
 * Mock `fetch` routed by "METHOD /path" (query string ignored). Unmatched
 * requests get a 404 error envelope. All calls are recorded in `calls`.
 */
export function mockFetch(routes: Record<string, MockRoute> = {}) {
  const calls: MockRequest[] = [];
  const table = { ...routes };

  const fn = vi.fn(async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, "http://localhost");
    const method = (init?.method ?? "GET").toUpperCase();
    const headers = new Headers(init?.headers);
    const body: unknown = typeof init?.body === "string" ? JSON.parse(init.body) : undefined;
    const req: MockRequest = { method, url, headers, body };
    calls.push(req);

    const route = table[`${method} ${decodeURI(url.pathname)}`];
    if (route === undefined) return apiError(404, "not_found", `no mock for ${method} ${url.pathname}`);
    const result = typeof route === "function" ? await (route as MockHandler)(req) : route;
    if (result instanceof Response) return result.clone();
    return json(result);
  });

  return {
    fetch: fn as unknown as typeof fetch,
    calls,
    /** Adds or replaces a route. */
    on(key: string, route: MockRoute) {
      table[key] = route;
    },
    /** Calls matching "METHOD /path". */
    find(key: string): MockRequest[] {
      return calls.filter((c) => `${c.method} ${decodeURI(c.url.pathname)}` === key);
    },
  };
}

export type FetchMock = ReturnType<typeof mockFetch>;
