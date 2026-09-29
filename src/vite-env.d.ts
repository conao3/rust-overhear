/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_OVERHEAR_GRAPHQL?: string;
  readonly VITE_OVERHEAR_WS?: string;
  readonly VITE_OVERHEAR_TOKEN?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
