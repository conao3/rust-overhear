/**
 * どこの音を拾うかの選択。
 *
 * 再生側 (SINK) を選べばその monitor を、録音側 (SOURCE) を選べば
 * そのまま拾う。既定に戻す選択肢を先頭に置く。
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

import type { AudioDevice } from "../lib/types";

const DEFAULT_KEY = "__default__";

type Props = {
  devices: AudioDevice[];
  selected: string | null;
  onChange: (deviceId: string | null) => void;
};

export function DevicePicker({ devices, selected, onChange }: Props) {
  return (
    <Select
      selectedKey={selected ?? DEFAULT_KEY}
      onSelectionChange={(key) =>
        onChange(key === DEFAULT_KEY ? null : String(key))
      }
      className="flex items-center gap-2"
    >
      <Label className="text-xs whitespace-nowrap text-ink-muted">音源</Label>
      <Button className="max-w-64 truncate rounded bg-white/10 px-2 py-1 text-sm outline-none data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-white/15">
        <SelectValue />
      </Button>
      <Popover className="max-w-96 rounded-lg border border-white/10 bg-surface-raised p-1 shadow-xl">
        <ListBox className="outline-none">
          <ListBoxItem
            id={DEFAULT_KEY}
            textValue="既定のシンク"
            className="cursor-pointer rounded px-2 py-1.5 text-sm outline-none data-[focused]:bg-accent/25"
          >
            既定のシンク (システム音声)
          </ListBoxItem>
          {devices.map((device) => (
            <ListBoxItem
              key={device.id}
              id={device.id}
              textValue={device.description}
              className="cursor-pointer rounded px-2 py-1.5 text-sm outline-none data-[focused]:bg-accent/25"
            >
              <div className="flex items-center gap-2">
                <span
                  className={
                    device.kind === "SINK"
                      ? "rounded bg-sky-500/20 px-1 text-[10px] text-sky-300"
                      : "rounded bg-purple-500/20 px-1 text-[10px] text-purple-300"
                  }
                >
                  {device.kind === "SINK" ? "再生" : "録音"}
                </span>
                <span className="truncate">{device.description}</span>
                {device.isDefault && (
                  <span className="text-[10px] text-ink-muted">既定</span>
                )}
              </div>
            </ListBoxItem>
          ))}
        </ListBox>
      </Popover>
    </Select>
  );
}
