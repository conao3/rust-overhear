/**
 * 訳が無い行の「訳す」ボタンと、翻訳中・失敗の表示。
 *
 * 既定のエンジンが「翻訳しない」のときは何も出さない。
 */
import { Button } from "react-aria-components";

import type { Segment } from "../lib/types";

type Props = {
  segment: Segment;
  translating: boolean;
  canTranslate: boolean;
  onTranslate: (segment: Segment) => void;
  className?: string;
};

export function TranslateAction({
  segment,
  translating,
  canTranslate,
  onTranslate,
  className,
}: Props) {
  if (!canTranslate || segment.status !== "FINAL") return null;
  if (translating) {
    return (
      <p className={`text-sm text-ink-muted/60 ${className ?? ""}`}>翻訳中…</p>
    );
  }
  return (
    <div className={`flex items-center gap-2 ${className ?? ""}`}>
      <Button
        className="rounded bg-white/10 px-2 py-1 text-sm text-ink-muted outline-none data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-white/20"
        onPress={() => onTranslate(segment)}
      >
        訳す
      </Button>
      {segment.translationError && (
        <span className="text-xs text-amber-300">
          翻訳できなかった ({segment.translationError})
        </span>
      )}
    </div>
  );
}
