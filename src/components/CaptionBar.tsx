/**
 * 常時最前面のキャプションバー相当の表示。
 * 原文 (語ごとにクリック可能) と訳を出す。
 */
import { formatMs } from "../lib/config";
import type { Segment } from "../lib/types";
import { WordPopover } from "./WordPopover";

type Props = {
  segment: Segment | null;
  onPlay?: (startMs: number) => void;
};

export function CaptionBar({ segment, onPlay }: Props) {
  if (!segment) {
    return (
      <div className="flex h-32 items-center justify-center text-ink-muted">
        音声を待っている…
      </div>
    );
  }

  const translation = segment.translations.at(-1);

  return (
    <div className="rounded-xl bg-surface-raised/70 p-5 shadow-lg ring-1 ring-white/5">
      <div className="mb-2 flex items-center gap-2 text-xs text-ink-muted">
        <span
          className={
            segment.status === "FINAL"
              ? "rounded bg-accent/25 px-1.5 py-0.5"
              : "rounded bg-white/10 px-1.5 py-0.5 animate-pulse"
          }
        >
          {segment.status === "FINAL" ? "確定" : "認識中"}
        </span>
        <span>{formatMs(segment.startMs)}</span>
        <span>· {segment.asrEngine}</span>
      </div>

      <p className="text-2xl leading-relaxed font-medium">
        {segment.tokens.length > 0
          ? segment.tokens.map((token) => (
              <span key={token.index}>
                <WordPopover token={token} onPlay={onPlay} />{" "}
              </span>
            ))
          : segment.sourceText}
      </p>

      {translation ? (
        <p className="mt-3 text-lg text-ink-muted">
          {translation.text}
          {translation.fallbackFrom && (
            <span className="ml-2 text-xs">
              ({translation.fallbackFrom} が使えず {translation.engineId}{" "}
              で代替)
            </span>
          )}
        </p>
      ) : (
        segment.status === "FINAL" && (
          <p className="mt-3 text-sm text-ink-muted/60">翻訳を待っている…</p>
        )
      )}
    </div>
  );
}
