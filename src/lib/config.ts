/**
 * バックエンドの接続情報。
 *
 * Tauri は起動時に生成したトークンとポートを window.__OVERHEAR__ で渡す。
 * vite dev で単体起動するときは環境変数 (または既定値) を使う。
 */
export type OverhearEndpoint = {
  graphql: string;
  websocket: string;
  token: string | null;
};

declare global {
  interface Window {
    __OVERHEAR__?: OverhearEndpoint;
  }
}

const fallback: OverhearEndpoint = {
  graphql:
    import.meta.env.VITE_OVERHEAR_GRAPHQL ?? "http://127.0.0.1:4747/graphql",
  websocket: import.meta.env.VITE_OVERHEAR_WS ?? "ws://127.0.0.1:4747/graphql",
  token: import.meta.env.VITE_OVERHEAR_TOKEN ?? null,
};

export const endpoint: OverhearEndpoint = window.__OVERHEAR__ ?? fallback;

/** GraphQL には音声バイナリを載せないため、実体はこちらから取る。 */
export function audioUrl(path: string): string {
  const url = new URL(path, endpoint.graphql);
  if (endpoint.token) {
    url.searchParams.set("token", endpoint.token);
  }
  return url.toString();
}

export function formatMs(ms: number): string {
  const total = Math.floor(ms / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}
