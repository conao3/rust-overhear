import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useSubscription } from "@apollo/client/react";

import { CaptionBar } from "./components/CaptionBar";
import { EnginePicker } from "./components/EnginePicker";
import { SegmentHistory } from "./components/SegmentHistory";
import { audioUrl, formatMs } from "./lib/config";
import {
  CAPTURE_STATE,
  RETRANSLATE,
  SEGMENTS,
  SEGMENT_UPDATES,
  TRANSLATION_ENGINES,
} from "./lib/queries";
import type { CaptureState, Segment, TranslationEngineInfo } from "./lib/types";

const HISTORY_LIMIT = 50;

export function App() {
  const [engineId, setEngineId] = useState<string | null>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);

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

  const play = useCallback((url: string) => {
    if (!audioRef.current) return;
    audioRef.current.src = audioUrl(url);
    void audioRef.current.play().catch(() => {
      // リングバッファから溢れていれば 404。UI は黙って何もしない。
    });
  }, []);

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
        onPlay={() => latest && play(latest.audioUrl)}
      />

      <section className="min-h-0 flex-1 overflow-y-auto">
        <SegmentHistory
          segments={[...segments].reverse()}
          onPlay={(segment) => play(segment.audioUrl)}
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
