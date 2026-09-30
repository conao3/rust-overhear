/**
 * 常時最前面の字幕バー。動画の上に重ねて使う。
 *
 * スタジオ (App) と違って操作を持たず、いま話されている文と訳だけを出す。
 * 語をクリックすれば辞書は引け、「訳す」で今の文を訳せる。保存や履歴はスタジオ側の仕事。
 */
import { useQuery, useSubscription } from "@apollo/client/react";
import { useMemo } from "react";

import { TranslateAction } from "./components/TranslateAction";
import { WordPopover } from "./components/WordPopover";
import { SEGMENTS, SEGMENT_UPDATES, TRANSLATION_ENGINES } from "./lib/queries";
import { displayText, displayWords } from "./lib/casing";
import { useTranslationRequests } from "./lib/translation";
import type { Segment, TranslationEngineInfo } from "./lib/types";

const RECENT_LIMIT = 2;

export function CaptionWindow() {
  const translationRequests = useTranslationRequests();
  const { data: engineData } = useQuery<{
    translationEngines: TranslationEngineInfo[];
  }>(TRANSLATION_ENGINES, {
    // スタジオで切り替えた既定エンジンは別ウィンドウのキャッシュに入らない。
    pollInterval: 10_000,
  });
  const defaultEngine = engineData?.translationEngines.find(
    (e) => e.isDefault,
  )?.id;
  const canTranslate = defaultEngine !== undefined && defaultEngine !== "none";
  const { data } = useQuery<{ segments: Segment[] }>(SEGMENTS, {
    variables: { limit: RECENT_LIMIT },
  });

  // 差し替えは Segment の正規化キャッシュが吸収するので、
  // ここで面倒を見るのは新しい id を一覧へ足すことだけ。
  useSubscription<{ segmentUpdates: Segment }>(SEGMENT_UPDATES, {
    onData: ({ data: incoming, client }) => {
      const segment = incoming.data?.segmentUpdates;
      if (!segment) return;
      client.cache.updateQuery<{ segments: Segment[] }>(
        { query: SEGMENTS, variables: { limit: RECENT_LIMIT } },
        (prev) => {
          const list = prev?.segments ?? [];
          if (list.some((s) => s.id === segment.id)) return prev;
          return { segments: [...list, segment].slice(-RECENT_LIMIT) };
        },
      );
    },
  });

  const segments = useMemo(() => data?.segments ?? [], [data]);
  const current = segments.at(-1) ?? null;
  const previous = segments.length > 1 ? segments.at(-2) : null;
  const translation = current?.translations.at(-1);

  return (
    <div className="flex h-full flex-col overflow-hidden rounded-xl bg-surface/90 ring-1 ring-white/10 backdrop-blur">
      {/* 枠が無いので、ここを掴んで動かす */}
      <div
        data-tauri-drag-region
        className="flex h-5 shrink-0 cursor-move items-center justify-center"
      >
        <div className="h-1 w-10 rounded-full bg-white/20" />
      </div>

      <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-1 px-6 pb-4 text-center">
        {previous && (
          <p className="max-w-full truncate text-sm text-ink-muted/50">
            {displayText(previous)}
          </p>
        )}

        {current ? (
          <>
            <p className="max-w-full text-2xl leading-snug font-medium text-balance">
              {current.tokens.length > 0
                ? displayWords(current).map((label, i) => (
                    <span key={current.tokens[i].index}>
                      <WordPopover
                        token={current.tokens[i]}
                        label={label}
                        segmentId={current.id}
                      />{" "}
                    </span>
                  ))
                : displayText(current)}
            </p>
            {translation ? (
              <p className="max-w-full truncate text-base text-ink-muted">
                {translation.text}
              </p>
            ) : (
              <TranslateAction
                segment={current}
                translating={translationRequests.isPending(current)}
                canTranslate={canTranslate}
                onTranslate={translationRequests.request}
              />
            )}
          </>
        ) : (
          <p className="text-center text-sm text-ink-muted">
            音声を待っている…
          </p>
        )}
      </div>
    </div>
  );
}
