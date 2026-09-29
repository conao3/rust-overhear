/**
 * 翻訳ストラテジーの選択。
 *
 * capabilities の sends_data_externally をそのまま UI に出し、
 * ローカルと外部送信を視覚的に分ける。availability が false の
 * エンジンは理由つきで非活性にする。
 */
import {
  Button,
  Label,
  ListBox,
  ListBoxItem,
  Popover,
  Select,
  SelectValue,
} from "react-aria-components";

import type { TranslationEngineInfo } from "../lib/types";

type Props = {
  engines: TranslationEngineInfo[];
  selected: string | null;
  onChange: (engineId: string) => void;
};

export function EnginePicker({ engines, selected, onChange }: Props) {
  const disabledKeys = engines.filter((e) => !e.available).map((e) => e.id);

  return (
    <Select
      selectedKey={selected}
      disabledKeys={disabledKeys}
      onSelectionChange={(key) => onChange(String(key))}
      className="flex items-center gap-2"
    >
      <Label className="text-xs text-ink-muted">翻訳エンジン</Label>
      <Button className="rounded bg-white/10 px-2 py-1 text-sm outline-none data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-white/15">
        <SelectValue />
      </Button>
      <Popover className="rounded-lg border border-white/10 bg-surface-raised p-1 shadow-xl">
        <ListBox className="outline-none">
          {engines.map((engine) => (
            <ListBoxItem
              key={engine.id}
              id={engine.id}
              textValue={engine.displayName}
              className="cursor-pointer rounded px-2 py-1.5 text-sm outline-none data-[disabled]:cursor-not-allowed data-[disabled]:opacity-40 data-[focused]:bg-accent/25"
            >
              <div className="flex items-center gap-2">
                <span>{engine.displayName}</span>
                {engine.sendsDataExternally ? (
                  <span className="rounded bg-amber-500/20 px-1 text-[10px] text-amber-300">
                    外部送信
                  </span>
                ) : (
                  <span className="rounded bg-emerald-500/20 px-1 text-[10px] text-emerald-300">
                    ローカル
                  </span>
                )}
                {engine.isDefault && (
                  <span className="text-[10px] text-ink-muted">既定</span>
                )}
              </div>
              {!engine.available && engine.unavailableReason && (
                <div className="text-[10px] text-ink-muted">
                  {engine.unavailableReason}
                </div>
              )}
            </ListBoxItem>
          ))}
        </ListBox>
      </Popover>
    </Select>
  );
}
