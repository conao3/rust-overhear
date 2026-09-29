/**
 * 字幕の 1 語。
 *
 * 字幕本体は react-aria-components のコレクションに載らないので、
 * 語そのものを button にして DialogTrigger の起点にしている。
 * 開いたときだけ辞書を引く (字幕は毎秒流れるので先読みしない)。
 */
import { useQuery } from "@apollo/client/react";
import { useState } from "react";
import { Button, Dialog, DialogTrigger, Popover } from "react-aria-components";

import { LOOKUP } from "../lib/queries";
import type { DictEntry, Token } from "../lib/types";

type Props = {
  token: Token;
  segmentId: string;
  onSave?: (segmentId: string, tokenIndex: number) => void;
  saving?: boolean;
};

export function WordPopover({ token, segmentId, onSave, saving }: Props) {
  const [isOpen, setOpen] = useState(false);
  const { data, loading } = useQuery<{ lookup: DictEntry[] }>(LOOKUP, {
    variables: { word: token.surface },
    skip: !isOpen,
  });

  const entries = data?.lookup ?? [];

  return (
    <DialogTrigger isOpen={isOpen} onOpenChange={setOpen}>
      <Button
        className="rounded px-0.5 outline-none transition-colors data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-accent/25 data-[pressed]:bg-accent/40"
        aria-label={`${token.surface} を調べる`}
      >
        {token.surface}
      </Button>
      <Popover className="max-w-md rounded-lg border border-white/10 bg-surface-raised p-4 shadow-xl">
        <Dialog className="outline-none">
          <div className="flex items-baseline justify-between gap-3">
            <span className="text-lg font-semibold">{token.surface}</span>
            {onSave && (
              <Button
                isDisabled={saving}
                className="rounded bg-accent/20 px-2 py-1 text-xs outline-none data-[disabled]:opacity-50 data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-accent/30"
                onPress={() => onSave(segmentId, token.index)}
              >
                語彙に保存
              </Button>
            )}
          </div>

          {loading && (
            <p className="mt-3 text-sm text-ink-muted">辞書を引いている…</p>
          )}

          {!loading && entries.length === 0 && (
            <p className="mt-3 text-sm text-ink-muted">
              辞書に見つからなかった
            </p>
          )}

          <div className="mt-3 flex max-h-72 flex-col gap-3 overflow-y-auto">
            {entries.map((entry) => (
              <div key={`${entry.lemma}-${entry.posLabel}`}>
                <div className="text-sm">
                  <span className="font-medium">{entry.lemma}</span>
                  <span className="ml-2 rounded bg-white/10 px-1 text-[10px] text-ink-muted">
                    {entry.posLabel}
                  </span>
                </div>
                <ol className="mt-1 list-decimal pl-5 text-sm text-ink-muted">
                  {entry.senses.slice(0, 3).map((sense, i) => (
                    <li key={i} className="mt-1">
                      {sense.definition}
                      {sense.synonyms.length > 0 && (
                        <span className="ml-1 opacity-60">
                          ＝ {sense.synonyms.join(", ")}
                        </span>
                      )}
                      {sense.examples[0] && (
                        <div className="mt-0.5 text-xs italic opacity-70">
                          「{sense.examples[0]}」
                        </div>
                      )}
                    </li>
                  ))}
                </ol>
              </div>
            ))}
          </div>
        </Dialog>
      </Popover>
    </DialogTrigger>
  );
}
