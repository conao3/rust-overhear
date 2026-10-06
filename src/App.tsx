import { useCallback, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useSubscription } from "@apollo/client/react";

import { Tab, TabList, TabPanel, Tabs } from "react-aria-components";

import { CaptionBar } from "./components/CaptionBar";
import { DevicePicker } from "./components/DevicePicker";
import { EnginePicker } from "./components/EnginePicker";
import { SegmentHistory } from "./components/SegmentHistory";
import { SettingsPanel } from "./components/SettingsPanel";
import { VocabList } from "./components/VocabList";
import {
  AudioUnavailableError,
  fetchAudioObjectUrl,
  formatDuration,
} from "./lib/config";
import {
  AUDIO_DEVICES,
  CAPTURE_STATE,
  EXPORT_TO_ANKI,
  MUTE_CAPTURE,
  REMOVE_VOCAB,
  RETRANSLATE,
  SAVE_VOCAB,
  SEGMENTS,
  SEGMENT_UPDATES,
  SET_CAPTURE_DEVICE,
  SET_DEFAULT_TRANSLATOR,
  TRANSLATION_ENGINES,
  VOCAB,
} from "./lib/queries";
import { useTranslationRequests } from "./lib/translation";
import type {
  AnkiExportResult,
  AudioDevice,
  CaptureState,
  Segment,
  Token,
  TranslationEngineInfo,
  VocabItem,
} from "./lib/types";

const HISTORY_LIMIT = 50;

export function App() {
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
  const translation = useTranslationRequests();
  const [setDefaultTranslator] = useMutation(SET_DEFAULT_TRANSLATOR);
  const [muteCapture] = useMutation(MUTE_CAPTURE);
  const { data: deviceData } = useQuery<{ audioDevices: AudioDevice[] }>(
    AUDIO_DEVICES,
  );
  const [setCaptureDevice] = useMutation(SET_CAPTURE_DEVICE);
  const { data: vocabData, refetch: refetchVocab } = useQuery<{
    vocab: VocabItem[];
  }>(VOCAB, { variables: { limit: 200 } });
  const [saveVocab, { loading: savingWord }] = useMutation(SAVE_VOCAB);
  const [removeVocab] = useMutation(REMOVE_VOCAB);
  const [exportToAnki, { loading: exporting }] = useMutation<{
    exportToAnki: AnkiExportResult;
  }>(EXPORT_TO_ANKI);
  const [vocabMessage, setVocabMessage] = useState<string | null>(null);

  // interim → final の差し替えは Segment の正規化キャッシュが吸収する。
  // ここで面倒を見るのは「新しい id を一覧へ足す」ことだけ。
  useSubscription<{ segmentUpdates: Segment }>(SEGMENT_UPDATES, {
    ignoreResults: true,
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
  const history = useMemo(() => segments.slice(0, -1).reverse(), [segments]);
  const engines = engineData?.translationEngines ?? [];
  const engineId = engines.find((e) => e.isDefault)?.id ?? null;
  const canTranslate = engineId !== null && engineId !== "none";

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
  const vocab = vocabData?.vocab ?? [];
  const devices = deviceData?.audioDevices ?? [];

  const onPlay = useCallback(
    (segment: Segment) =>
      void play(segment.audioUrl, segment.endMs - segment.startMs),
    [play],
  );

  const onRetranslate = useCallback(
    (segment: Segment) => {
      void retranslate({ variables: { segmentId: segment.id, engineId } });
    },
    [retranslate, engineId],
  );

  const onSaveWord = useCallback(
    async (segment: Segment, token: Token) => {
      setVocabMessage(null);
      try {
        const res = await saveVocab({
          variables: {
            segmentId: segment.id,
            tokenIndex: token.index,
            sourceText: segment.sourceText,
            surface: token.surface,
          },
        });
        await refetchVocab();
        const saved = (res.data as { saveVocab?: VocabItem } | null | undefined)
          ?.saveVocab;
        setVocabMessage(saved ? `「${saved.lemma}」を語彙に保存した` : null);
      } catch (err) {
        setVocabMessage(
          `保存できなかった (${err instanceof Error ? err.message : String(err)})`,
        );
        throw err;
      }
    },
    [refetchVocab, saveVocab],
  );

  const onExport = useCallback(
    async (ids: string[]) => {
      setVocabMessage(null);
      try {
        const res = await exportToAnki({
          variables: { vocabIds: ids, deck: "overhear" },
        });
        await refetchVocab();
        const result = res.data?.exportToAnki;
        if (!result) return;
        setVocabMessage(
          result.failures.length > 0
            ? `${result.exported.length} 件を書き出し、${result.failures.length} 件が失敗: ${result.failures[0].reason}`
            : `${result.exported.length} 件を Anki へ書き出した`,
        );
      } catch (err) {
        setVocabMessage(
          `書き出せなかった (${err instanceof Error ? err.message : String(err)})`,
        );
      }
    },
    [exportToAnki, refetchVocab],
  );

  return (
    <div className="mx-auto flex h-full max-w-4xl flex-col gap-4 p-6">
      <header className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
        <div className="min-w-0">
          <h1 className="text-xl font-semibold">overhear</h1>
          <p className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-ink-muted">
            {captureState ? (
              <span className="inline-flex items-center gap-1.5">
                <span
                  aria-hidden="true"
                  className={`h-1.5 w-1.5 rounded-full ${captureState.running ? "bg-accent" : "bg-amber-400"}`}
                />
                {captureState.running
                  ? "音声を取得中"
                  : "音声に再接続しています…"}
              </span>
            ) : (
              "接続中…"
            )}
            {captureState && captureState.translationBacklog > 0 && (
              <span>翻訳待ち {captureState.translationBacklog} 件</span>
            )}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <DevicePicker
            devices={devices}
            selected={captureState?.captureTarget ?? null}
            onChange={(deviceId) => {
              void setCaptureDevice({
                variables: { deviceId },
                refetchQueries: [CAPTURE_STATE],
              });
            }}
          />
          <EnginePicker
            engines={engines}
            selected={engineId}
            onChange={(id) => {
              void setDefaultTranslator({ variables: { engineId: id } });
            }}
          />
        </div>
      </header>

      <CaptionBar
        segment={latest}
        onSaveWord={onSaveWord}
        savingWord={savingWord}
        onTranslate={translation.request}
        translating={latest ? translation.isPending(latest) : false}
        canTranslate={canTranslate}
      />

      {playError && (
        <p className="rounded bg-amber-500/15 px-3 py-2 text-sm text-amber-200">
          {playError}
        </p>
      )}

      <Tabs className="flex min-h-0 flex-1 flex-col gap-3">
        <TabList aria-label="表示の切り替え" className="flex gap-1">
          {[
            { id: "history", label: `履歴 (${history.length})` },
            { id: "vocab", label: `語彙 (${vocab.length})` },
            { id: "settings", label: "設定" },
          ].map((tab) => (
            <Tab
              key={tab.id}
              id={tab.id}
              className="cursor-pointer rounded px-3 py-1.5 text-sm outline-none data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-white/10 data-[selected]:bg-accent/20"
            >
              {tab.label}
            </Tab>
          ))}
        </TabList>

        <TabPanel
          id="history"
          className="min-h-0 flex-1 overflow-y-auto outline-none"
        >
          {captureState && (
            <p className="mb-3 text-xs leading-relaxed text-ink-muted">
              各行の「聞き直す」から音声を再生できます。 音声は最大
              {formatDuration(captureState.ringSeconds * 1000)}
              間保持され、古いものから自動で削除されます。
            </p>
          )}
          <SegmentHistory
            audioAvailableFromMs={
              captureState
                ? Math.max(0, captureState.capturedMs - captureState.bufferedMs)
                : 0
            }
            segments={history}
            onPlay={onPlay}
            onTranslate={translation.request}
            isTranslating={translation.isPending}
            canTranslate={canTranslate}
            onSaveWord={onSaveWord}
            savingWord={savingWord}
            onRetranslate={onRetranslate}
          />
        </TabPanel>

        <TabPanel
          id="vocab"
          className="min-h-0 flex-1 overflow-y-auto outline-none"
        >
          <VocabList
            items={vocab}
            exporting={exporting}
            message={vocabMessage}
            onExport={(ids) => void onExport(ids)}
            onRemove={(id) => {
              void removeVocab({ variables: { id } }).then(() =>
                refetchVocab(),
              );
            }}
          />
        </TabPanel>
        <TabPanel
          id="settings"
          className="min-h-0 flex-1 overflow-y-auto outline-none"
        >
          <SettingsPanel />
          {captureState && (
            <details className="mt-6 border-t border-white/10 pt-4 text-xs text-ink-muted">
              <summary className="w-fit cursor-pointer rounded outline-none focus-visible:ring-2 focus-visible:ring-accent">
                動作情報
              </summary>
              <dl className="mt-3 grid grid-cols-[auto_1fr] gap-x-6 gap-y-2">
                <dt>音声認識</dt>
                <dd>{captureState.asrEngine}</dd>
                {captureState.refiner && (
                  <>
                    <dt>認識結果の補正</dt>
                    <dd>{captureState.refiner}</dd>
                  </>
                )}
                <dt>サンプルレート</dt>
                <dd>{captureState.sampleRate.toLocaleString()} Hz</dd>
                <dt>音声の保持期間</dt>
                <dd>最大{formatDuration(captureState.ringSeconds * 1000)}</dd>
              </dl>
            </details>
          )}
        </TabPanel>
      </Tabs>

      <audio ref={audioRef} hidden />
    </div>
  );
}
