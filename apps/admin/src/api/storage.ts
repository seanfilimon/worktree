/**
 * Small key/value persistence over localStorage that never throws: storage
 * can be unavailable (privacy mode, blocked site data, sandboxed iframes), in
 * which case values live in memory for the lifetime of the page.
 */
export interface KeyValueStore {
  get(): string | null;
  set(value: string | null): void;
}

export function createPersistentValue(key: string, storage?: Storage): KeyValueStore {
  let memory: string | null = null;
  const backend = (): Storage | undefined => {
    try {
      return storage ?? globalThis.localStorage;
    } catch {
      return undefined;
    }
  };

  return {
    get() {
      try {
        const value = backend()?.getItem(key);
        if (value !== undefined && value !== null) return value;
      } catch {
        // fall through to the in-memory copy
      }
      return memory;
    },
    set(value) {
      memory = value;
      try {
        const s = backend();
        if (value === null) s?.removeItem(key);
        else s?.setItem(key, value);
      } catch {
        // keep the in-memory copy only
      }
    },
  };
}

export const TOKEN_STORAGE_KEY = "wt.admin.token";
/** v0.8 device-auth (§16.6): the refresh token renewing the access token. */
export const REFRESH_STORAGE_KEY = "wt.admin.refresh";
export const THEME_STORAGE_KEY = "wt.admin.theme";
