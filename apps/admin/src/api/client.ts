import type { KeyValueStore } from "./storage";

/** Error codes from the REST error envelope (§7), plus client-side ones. */
export type ApiErrorCode =
  | "bad_request"
  | "unauthenticated"
  | "forbidden"
  | "not_found"
  | "conflict"
  | "internal"
  | "network"
  | (string & {});

const STATUS_CODES: Record<number, ApiErrorCode> = {
  400: "bad_request",
  401: "unauthenticated",
  403: "forbidden",
  404: "not_found",
  409: "conflict",
};

/** A failed API call. `details` carries any extra fields of the envelope. */
export class ApiError extends Error {
  readonly status: number;
  readonly code: ApiErrorCode;
  readonly details: Record<string, unknown>;

  constructor(status: number, code: ApiErrorCode, message: string, details: Record<string, unknown> = {}) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
    this.details = details;
  }
}

export function isApiError(err: unknown): err is ApiError {
  return err instanceof ApiError;
}

/** Human-readable message for any thrown value. */
export function errorMessage(err: unknown): string {
  if (err instanceof Error && err.message) return err.message;
  return "Something went wrong";
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

/** Parses `{"error":{"code","message",...}}`, falling back to the status. */
export async function parseErrorResponse(res: Response): Promise<ApiError> {
  const fallbackCode = STATUS_CODES[res.status] ?? (res.status >= 500 ? "internal" : "unknown");
  const fallbackMessage = res.statusText || `Request failed with status ${res.status}`;
  let text: string;
  try {
    text = await res.text();
  } catch {
    return new ApiError(res.status, fallbackCode, fallbackMessage);
  }
  try {
    const body: unknown = JSON.parse(text);
    if (isRecord(body) && isRecord(body.error)) {
      const { code, message, ...rest } = body.error;
      return new ApiError(
        res.status,
        typeof code === "string" && code ? code : fallbackCode,
        typeof message === "string" && message ? message : fallbackMessage,
        rest,
      );
    }
  } catch {
    // not JSON: use the raw text below
  }
  const trimmed = text.trim();
  return new ApiError(res.status, fallbackCode, trimmed && trimmed.length < 300 ? trimmed : fallbackMessage);
}

export type QueryValue = string | number | boolean | null | undefined;

export interface RequestOptions {
  method?: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
  query?: Record<string, QueryValue>;
  body?: unknown;
  signal?: AbortSignal;
  /** Do not treat 401 as "session expired" (login/register calls). */
  skipAuthRedirect?: boolean;
}

export interface ApiClientOptions {
  /** Origin (or prefix) of the server; "" = same origin. */
  baseUrl?: string;
  tokenStore: KeyValueStore;
  /** Defaults to the global fetch, resolved at call time. */
  fetch?: typeof fetch;
}

/** Encodes each path segment; keeps "/" separators. */
export function encodePath(path: string): string {
  return path
    .split("/")
    .filter((s) => s !== "")
    .map(encodeURIComponent)
    .join("/");
}

export function buildQuery(query: Record<string, QueryValue> | undefined): string {
  if (!query) return "";
  const params = new URLSearchParams();
  for (const [k, v] of Object.entries(query)) {
    if (v === undefined || v === null || v === "") continue;
    params.set(k, String(v));
  }
  const s = params.toString();
  return s ? `?${s}` : "";
}

type UnauthorizedListener = () => void;

/**
 * Thin typed wrapper around fetch: bearer auth, JSON bodies, error envelope
 * parsing and a session-expired notification on 401.
 */
export class ApiClient {
  readonly baseUrl: string;
  private readonly tokens: KeyValueStore;
  private readonly fetchImpl: typeof fetch | undefined;
  private readonly listeners = new Set<UnauthorizedListener>();

  constructor(opts: ApiClientOptions) {
    this.baseUrl = (opts.baseUrl ?? "").replace(/\/+$/, "");
    this.tokens = opts.tokenStore;
    this.fetchImpl = opts.fetch;
  }

  get token(): string | null {
    return this.tokens.get();
  }

  setToken(token: string | null): void {
    this.tokens.set(token);
  }

  /** Subscribe to "session expired" (401 on an authenticated call). */
  onUnauthorized(listener: UnauthorizedListener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  url(path: string, query?: Record<string, QueryValue>): string {
    return `${this.baseUrl}${path}${buildQuery(query)}`;
  }

  /** Performs the request and returns the raw successful Response. */
  async raw(path: string, opts: RequestOptions = {}): Promise<Response> {
    const headers = new Headers({ Accept: "application/json" });
    const token = this.token;
    if (token) headers.set("Authorization", `Bearer ${token}`);
    let body: BodyInit | undefined;
    if (opts.body !== undefined) {
      headers.set("Content-Type", "application/json");
      body = JSON.stringify(opts.body);
    }

    const doFetch = this.fetchImpl ?? globalThis.fetch.bind(globalThis);
    let res: Response;
    try {
      res = await doFetch(this.url(path, opts.query), {
        method: opts.method ?? "GET",
        headers,
        body,
        signal: opts.signal,
      });
    } catch (err) {
      if (err instanceof DOMException && err.name === "AbortError") throw err;
      throw new ApiError(0, "network", "Cannot reach the W0rkTree server. Check that it is running.");
    }

    if (!res.ok) {
      const error = await parseErrorResponse(res);
      if (res.status === 401 && !opts.skipAuthRedirect) {
        this.setToken(null);
        for (const l of [...this.listeners]) l();
      }
      throw error;
    }
    return res;
  }

  /** JSON request; resolves `undefined` for 204 / empty bodies. */
  async request<T>(path: string, opts: RequestOptions = {}): Promise<T> {
    const res = await this.raw(path, opts);
    if (res.status === 204) return undefined as T;
    const text = await res.text();
    if (!text) return undefined as T;
    try {
      return JSON.parse(text) as T;
    } catch {
      throw new ApiError(res.status, "internal", "The server returned an invalid JSON response.");
    }
  }
}
