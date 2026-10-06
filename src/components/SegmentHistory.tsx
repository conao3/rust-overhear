/**
 * 発話の履歴。行ごとに聞き直しと翻訳 (訳が無ければ訳す、あれば引き直す) ができる。
 *
 * 行の中にボタンを置くため ListBox ではなく GridList を使う
 * (GridList は行内のフォーカス可能な要素を扱える)。
 */
import { memo } from "react";
import { Button, GridList, GridListItem } from "react-aria-components";

import { formatMs } from "../lib/config";
import type { Segment, Token } from "../lib/types";
import { SegmentWords } from "./SegmentWords";

type Props = {
  segments: Segment[];
  audioAvailableFromMs: number;
  onPlay: (segment: Segment) => void;
  onTranslate: (segment: Segment) => void;
  onRetranslate: (segment: Segment) => void;
  isTranslating: (segment: Segment) => boolean;
  canTranslate: boolean;
  onSaveWord: (segment: Segment, token: Token) => Promise<void>;
  savingWord: boolean;
};

export const SegmentHistory = memo(
  function SegmentHistory({
    segments,
    audioAvailableFromMs,
    onPlay,
    onTranslate,
    onRetranslate,
    isTranslating,
    canTranslate,
    onSaveWord,
    savingWord,
  }: Props) {
    return (
      <GridList
        aria-label="発話の履歴"
        items={segments}
        dependencies={[
          canTranslate,
          isTranslating,
          onSaveWord,
          savingWord,
          audioAvailableFromMs,
          onPlay,
          onTranslate,
          onRetranslate,
        ]}
        className="flex flex-col gap-2 outline-none"
        renderEmptyState={() => (
          <div className="py-8 text-center text-sm text-ink-muted">
            過去の発話がここに表示されます
          </div>
        )}
      >
        {(segment) => (
          <GridListItem
            key={segment.id}
            id={segment.id}
            textValue={segment.sourceText}
            className="rounded-lg bg-surface-raised/50 p-3 outline-none ring-1 ring-white/5 data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-surface-raised"
          >
            <div className="flex items-start gap-3">
              <span className="mt-0.5 shrink-0 font-mono text-xs text-ink-muted">
                {formatMs(segment.startMs)}
              </span>
              <div className="min-w-0 flex-1">
                <p className="text-sm">
                  <SegmentWords
                    segment={segment}
                    onSaveWord={onSaveWord}
                    savingWord={savingWord}
                  />
                </p>
                {segment.translations.map((t) => (
                  <p key={t.engineId} className="mt-1 text-sm text-ink-muted">
                    <span className="mr-1 text-[10px] uppercase opacity-60">
                      {t.engineId}
                    </span>
                    {t.text}
                  </p>
                ))}
                {segment.translationError && (
                  <p className="mt-1 text-xs text-amber-300">
                    翻訳できなかった ({segment.translationError})
                  </p>
                )}
              </div>
              <div className="flex shrink-0 gap-1">
                <Button
                  isDisabled={segment.startMs < audioAvailableFromMs}
                  aria-label={
                    segment.startMs < audioAvailableFromMs
                      ? "保持期間を過ぎたため、この音声は再生できません"
                      : "この発話を聞き直す"
                  }
                  className="rounded bg-white/10 px-2 py-1 text-xs outline-none data-[disabled]:cursor-default data-[disabled]:bg-transparent data-[disabled]:text-ink-muted data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-white/20"
                  onPress={() => onPlay(segment)}
                >
                  {segment.startMs < audioAvailableFromMs
                    ? "音声なし"
                    : "聞き直す"}
                </Button>
                {canTranslate && (
                  <Button
                    isDisabled={isTranslating(segment)}
                    className="rounded bg-white/10 px-2 py-1 text-xs outline-none data-[disabled]:opacity-50 data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-white/20"
                    onPress={() =>
                      segment.translations.length > 0
                        ? onRetranslate(segment)
                        : onTranslate(segment)
                    }
                  >
                    {isTranslating(segment)
                      ? "翻訳中…"
                      : segment.translations.length > 0
                        ? "訳し直す"
                        : "訳す"}
                  </Button>
                )}
              </div>
            </div>
          </GridListItem>
        )}
      </GridList>
    );
  },
  (prev, next) =>
    prev.segments.length === next.segments.length &&
    prev.segments.every((segment, index) => segment === next.segments[index]) &&
    prev.audioAvailableFromMs === next.audioAvailableFromMs &&
    prev.onPlay === next.onPlay &&
    prev.onTranslate === next.onTranslate &&
    prev.onRetranslate === next.onRetranslate &&
    prev.isTranslating === next.isTranslating &&
    prev.canTranslate === next.canTranslate &&
    prev.onSaveWord === next.onSaveWord &&
    prev.savingWord === next.savingWord,
);
