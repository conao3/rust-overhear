import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useSubscription } from "@apollo/client/react";

import { CaptionBar } from "./components/CaptionBar";
import { EnginePicker } from "./components/EnginePicker";
import { SegmentHistory } from "./components/SegmentHistory";
import {
  AudioUnavailableError,
  fetchAudioObjectUrl,
  formatMs,
} from "./lib/config";
import {
  CAPTURE_STATE,
  MUTE_CAPTURE,
  RETRANSLATE,
  SEGMENTS,
  SEGMENT_UPDATES,
  TRANSLATION_ENGINES,
} from "./lib/queries";
import type { CaptureState, Segment, TranslationEngineInfo } from "./lib/types";

const HISTORY_LIMIT = 50;

export function App() {
  const [engineId, setEngineId] = useState<string | null>(null);
  const [playError, setPlayError] = useState<string | null>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const objectUrlRef = useRef<string | null>(null);

  const { data: segmentsData } = useQuery<{ segments: Segment[] }>(SEGMENTS, {
    variables: { limit: HISTORY_LIMIT },
  });
  const { data: engineData } = useQuery<{
    translationEngines: TranslationEngineInfo[];
  }>(TRANSLATION_ENGINES);
  const { data: stateData } = useQuery<{ captureState: CaptureState }>(
    CAPTURE_STATE,
    {
      pollInterval: 5000,
    },
  );
  const [retranslate] = useMutation(RETRANSLATE);
  const [muteCapture] = useMutation(MUTE_CAPTURE);

  // interim → final の差し替えは Segment の正規化キャッシュが吸収する。
  // ここで面倒を見るのは「新しい id を一覧へ足す」ことだけ。
  useSubscription<{ segmentUpdates: Segment }>(SEGMENT_UPDATES, {
    onData: ({ data, client }) => {
      const incoming = data.data?.segmentUpdates;
      if (!incoming) return;
      client.cache.updateQuery<{ segments: Segment[] }>(
        { query: SEGMENTS, variables: { limit: HISTORY_LIMIT } },
        (prev) => {
          const list = prev?.segments ?? [];
          if (list.some((s) => s.id === incoming.id)) return prev;
          return { segments: [...list, incoming].slice(-HISTORY_LIMIT) };
        },
      );
    },
  });

  const segments = useMemo(() => segmentsData?.segments ?? [], [segmentsData]);
  const latest = segments.at(-1) ?? null;
  const engines = engineData?.translationEngines ?? [];

  useEffect(() => {
    if (engineId === null && engines.length > 0) {
      setEngineId(engines.find((e) => e.isDefault)?.id ?? engines[0].id);
    }
  }, [engineId, engines]);

  const play = useCallback(
    async (url: string, durationMs: number) => {
      if (!audioRef.current) return;
      setPlayError(null);
      // 再生音は既定シンクの monitor に戻ってくる。再生している間は
      // 入力を閉じておかないと、聞き直すたびに履歴が汚れる。
      await muteCapture({ variables: { ms: durationMs + 800 } }).catch(
        () => {},
      );
      try {
        // 前回の blob URL を解放してから差し替える。
        if (objectUrlRef.current) URL.revokeObjectURL(objectUrlRef.current);
        const objectUrl = await fetchAudioObjectUrl(url);
        objectUrlRef.current = objectUrl;
        audioRef.current.src = objectUrl;
        await audioRef.current.play();
      } catch (err) {
        setPlayError(
          err instanceof AudioUnavailableError
            ? err.message
            : `再生できなかった (${err instanceof Error ? err.message : String(err)})`,
        );
      }
    },
    [muteCapture],
  );

  const captureState = stateData?.captureState;

  return (
    <div className="mx-auto flex h-full max-w-4xl flex-col gap-4 p-6">
      <header className="flex items-center justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold">overhear</h1>
          {captureState && (
            <p className="text-xs text-ink-muted">
              {captureState.asrEngine} · {captureState.sampleRate} Hz · バッファ{" "}
              {formatMs(captureState.capturedMs)} / {captureState.ringSeconds}{" "}
              秒 · 訳先 {captureState.targetLang}
            </p>
          )}
        </div>
        <EnginePicker
          engines={engines}
          selected={engineId}
          onChange={setEngineId}
        />
      </header>

      <CaptionBar
        segment={latest}
        onPlay={() =>
          latest && void play(latest.audioUrl, latest.endMs - latest.startMs)
        }
      />

      {playError && (
        <p className="rounded bg-amber-500/15 px-3 py-2 text-sm text-amber-200">
          {playError}
        </p>
      )}

      <section className="min-h-0 flex-1 overflow-y-auto">
        <SegmentHistory
          segments={[...segments].reverse()}
          onPlay={(segment) =>
            void play(segment.audioUrl, segment.endMs - segment.startMs)
          }
          onRetranslate={(segment) => {
            void retranslate({
              variables: { segmentId: segment.id, engineId },
            });
          }}
        />
      </section>

      <audio ref={audioRef} hidden />
    </div>
  );
}
