/**
 * 発話の履歴。行ごとに聞き直しと翻訳 (訳が無ければ訳す、あれば引き直す) ができる。
 *
 * 行の中にボタンを置くため ListBox ではなく GridList を使う
 * (GridList は行内のフォーカス可能な要素を扱える)。
 */
import { Button, GridList, GridListItem } from "react-aria-components";

import { displayText } from "../lib/casing";
import { formatMs } from "../lib/config";
import type { Segment } from "../lib/types";

type Props = {
  segments: Segment[];
  onPlay: (segment: Segment) => void;
  onTranslate: (segment: Segment) => void;
  onRetranslate: (segment: Segment) => void;
  isTranslating: (segment: Segment) => boolean;
  canTranslate: boolean;
};

export function SegmentHistory({
  segments,
  onPlay,
  onTranslate,
  onRetranslate,
  isTranslating,
  canTranslate,
}: Props) {
  return (
    <GridList
      aria-label="発話の履歴"
      items={segments}
      dependencies={[canTranslate, isTranslating]}
      className="flex flex-col gap-2 outline-none"
      renderEmptyState={() => (
        <div className="py-8 text-center text-sm text-ink-muted">
          まだ発話がない
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
              <p className="text-sm">{displayText(segment)}</p>
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
                className="rounded bg-white/10 px-2 py-1 text-xs outline-none data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-white/20"
                onPress={() => onPlay(segment)}
              >
                再生
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
}
