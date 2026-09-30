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
  formatMs,
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

  const onSaveWord = useCallback(
    async (segmentId: string, tokenIndex: number) => {
      setVocabMessage(null);
      try {
        const res = await saveVocab({ variables: { segmentId, tokenIndex } });
        await refetchVocab();
        const saved = (res.data as { saveVocab?: VocabItem } | null | undefined)
          ?.saveVocab;
        setVocabMessage(saved ? `「${saved.lemma}」を語彙に保存した` : null);
      } catch (err) {
        setVocabMessage(
          `保存できなかった (${err instanceof Error ? err.message : String(err)})`,
        );
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
          {captureState && (
            <p className="text-xs text-ink-muted">
              {captureState.asrEngine}
              {captureState.refiner && ` + ${captureState.refiner}`} ·{" "}
              {captureState.sampleRate} Hz · バッファ{" "}
              {formatMs(captureState.capturedMs)} / {captureState.ringSeconds}{" "}
              秒 · 訳先 {captureState.targetLang}
              {captureState.translationBacklog > 0 &&
                ` · 翻訳待ち ${captureState.translationBacklog}`}
              {!captureState.running && (
                <span className="ml-2 rounded bg-amber-500/20 px-1.5 text-amber-300">
                  音声を拾えていない (再接続している)
                </span>
              )}
            </p>
          )}
        </div>
        <div className="flex items-center gap-4">
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
            { id: "history", label: `履歴 (${segments.length})` },
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
          <SegmentHistory
            segments={[...segments].reverse()}
            onPlay={(segment) =>
              void play(segment.audioUrl, segment.endMs - segment.startMs)
            }
            onTranslate={translation.request}
            isTranslating={translation.isPending}
            canTranslate={canTranslate}
            onRetranslate={(segment) => {
              void retranslate({
                variables: { segmentId: segment.id, engineId },
              });
            }}
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
        </TabPanel>
      </Tabs>

      <audio ref={audioRef} hidden />
    </div>
  );
}
