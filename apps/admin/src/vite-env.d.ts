/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Optional absolute server origin; default is same origin (dev proxy / Go server). */
  readonly VITE_WT_SERVER_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
