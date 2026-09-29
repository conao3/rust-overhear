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
  return new URL(path, endpoint.graphql).toString();
}

export class AudioUnavailableError extends Error {}

/**
 * segment の音声を取得して blob URL にする。
 *
 * `<audio src>` は Authorization ヘッダを付けられないので、fetch で取ってから
 * 再生する。トークンを URL のクエリに載せない方針。
 */
export async function fetchAudioObjectUrl(path: string): Promise<string> {
  const res = await fetch(audioUrl(path), {
    headers: endpoint.token
      ? { Authorization: `Bearer ${endpoint.token}` }
      : {},
  });
  if (res.status === 404) {
    throw new AudioUnavailableError("この音声はリングバッファから溢れている");
  }
  if (!res.ok) {
    throw new Error(`音声を取得できない (HTTP ${res.status})`);
  }
  return URL.createObjectURL(await res.blob());
}

export function formatMs(ms: number): string {
  const total = Math.floor(ms / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}
