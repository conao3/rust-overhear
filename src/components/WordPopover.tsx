/**
 * 字幕の 1 語。
 *
 * 字幕本体は react-aria-components のコレクションに載らないので、
 * 語そのものを button にして DialogTrigger の起点にしている。
 * 辞書引き (WordNet / ejdict) はフェーズ 3 で、ここに差し込む。
 */
import { Button, Dialog, DialogTrigger, Popover } from "react-aria-components";

import { formatMs } from "../lib/config";
import type { Token } from "../lib/types";

type Props = {
  token: Token;
  onPlay?: (startMs: number) => void;
};

export function WordPopover({ token, onPlay }: Props) {
  return (
    <DialogTrigger>
      <Button
        className="rounded px-0.5 outline-none transition-colors data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-accent/25 data-[pressed]:bg-accent/40"
        aria-label={`${token.surface} を調べる`}
      >
        {token.surface}
      </Button>
      <Popover className="rounded-lg border border-white/10 bg-surface-raised p-3 shadow-xl entering:animate-in">
        <Dialog className="min-w-56 outline-none">
          <div className="text-lg font-semibold">{token.surface}</div>
          <div className="mt-1 text-xs text-ink-muted">
            この発話の {formatMs(token.startMs)} 地点
          </div>
          <div className="mt-3 rounded bg-black/20 p-2 text-xs text-ink-muted">
            辞書 (WordNet / ejdict) はフェーズ 3。ここに語義が入る。
          </div>
          {onPlay && (
            <Button
              className="mt-3 w-full rounded bg-accent/20 px-2 py-1 text-sm outline-none data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-accent/30"
              onPress={() => onPlay(token.startMs)}
            >
              ここから聞き直す
            </Button>
          )}
        </Dialog>
      </Popover>
    </DialogTrigger>
  );
}
