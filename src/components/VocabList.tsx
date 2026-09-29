/**
 * 保存した語彙。選んで Anki へ書き出す。
 *
 * 行の中にボタンを置き、複数選択もするため GridList を使う。
 */
import { useState } from "react";
import {
  Button,
  GridList,
  GridListItem,
  Selection,
} from "react-aria-components";

import type { VocabItem } from "../lib/types";

type Props = {
  items: VocabItem[];
  onExport: (ids: string[]) => void;
  onRemove: (id: string) => void;
  exporting?: boolean;
  message?: string | null;
};

export function VocabList({
  items,
  onExport,
  onRemove,
  exporting,
  message,
}: Props) {
  const [selected, setSelected] = useState<Selection>(new Set());

  const selectedIds =
    selected === "all"
      ? items.map((i) => i.id)
      : Array.from(selected).map(String);

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-3">
        <Button
          isDisabled={selectedIds.length === 0 || exporting}
          className="rounded bg-accent/20 px-3 py-1.5 text-sm outline-none data-[disabled]:opacity-40 data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-accent/30"
          onPress={() => onExport(selectedIds)}
        >
          Anki へ書き出す{selectedIds.length > 0 && ` (${selectedIds.length})`}
        </Button>
        <span className="text-xs text-ink-muted">{items.length} 語</span>
        {message && <span className="text-xs text-amber-200">{message}</span>}
      </div>

      <GridList
        aria-label="保存した語彙"
        items={items}
        selectionMode="multiple"
        selectedKeys={selected}
        onSelectionChange={setSelected}
        className="flex flex-col gap-2 outline-none"
        renderEmptyState={() => (
          <div className="py-8 text-center text-sm text-ink-muted">
            まだ語彙がない。字幕の単語をクリックして保存する
          </div>
        )}
      >
        {(item) => (
          <GridListItem
            key={item.id}
            id={item.id}
            textValue={item.lemma}
            className="rounded-lg bg-surface-raised/50 p-3 outline-none ring-1 ring-white/5 data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-surface-raised data-[selected]:bg-accent/15"
          >
            <div className="flex items-start gap-3">
              <div className="min-w-0 flex-1">
                <div className="flex items-baseline gap-2">
                  <span className="font-medium">{item.lemma}</span>
                  {item.surface.toLowerCase() !== item.lemma && (
                    <span className="text-xs text-ink-muted">
                      ({item.surface})
                    </span>
                  )}
                  {item.hasAudio && (
                    <span className="rounded bg-white/10 px-1 text-[10px] text-ink-muted">
                      音声あり
                    </span>
                  )}
                  {item.ankiNoteId && (
                    <span className="rounded bg-emerald-500/20 px-1 text-[10px] text-emerald-300">
                      Anki 済
                    </span>
                  )}
                </div>
                {item.definition && (
                  <p className="mt-1 text-sm text-ink-muted">
                    {item.definition}
                  </p>
                )}
                <p className="mt-1 text-xs opacity-60">{item.sentence}</p>
              </div>
              <Button
                className="shrink-0 rounded bg-white/10 px-2 py-1 text-xs outline-none data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-white/20"
                onPress={() => onRemove(item.id)}
              >
                削除
              </Button>
            </div>
          </GridListItem>
        )}
      </GridList>
    </div>
  );
}
