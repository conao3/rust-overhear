import { useEffect, useState } from "react";
import {
  Label,
  Slider,
  SliderOutput,
  SliderThumb,
  SliderTrack,
  Switch,
} from "react-aria-components";

import { useRuby } from "../lib/ruby";

export function RubyControls() {
  const { settings, save, saving, error } = useRuby();
  const [size, setSize] = useState(75);
  useEffect(() => {
    if (settings) setSize(settings.size);
  }, [settings?.size]);
  if (!settings)
    return (
      <p role="status" className="text-sm text-ink-muted">
        {error ?? "ルビの設定を読み込み中…"}
      </p>
    );
  return (
    <section className="flex flex-col gap-3">
      <h2 className="text-sm font-semibold">日本語ルビ</h2>
      <Switch
        isSelected={settings.enabled}
        isDisabled={saving}
        onChange={(enabled) =>
          void save({ ...settings, enabled }).catch(() => {})
        }
        className="group flex w-fit cursor-pointer items-center gap-3 text-sm outline-none data-[disabled]:opacity-50"
      >
        <span className="flex h-5 w-9 items-center rounded-full bg-white/20 p-0.5 group-data-[selected]:bg-accent group-data-[focus-visible]:ring-2 group-data-[focus-visible]:ring-accent">
          <span className="h-4 w-4 rounded-full bg-white group-data-[selected]:translate-x-4" />
        </span>
        色付きの単語に日本語ルビを表示
      </Switch>
      <label className="flex items-center gap-3 text-sm">
        <span className="w-32 text-ink-muted">単語レベル</span>
        <select
          aria-label="単語レベル"
          value={settings.knownWords}
          disabled={saving}
          onChange={(event) =>
            void save({
              ...settings,
              knownWords: Number(event.target.value),
            }).catch(() => {})
          }
          className="rounded bg-surface-raised px-2 py-1 outline-none focus-visible:ring-2 focus-visible:ring-accent"
        >
          {[0, 1000, 2000, 3000, 5000, 10000].map((count) => (
            <option key={count} value={count}>
              {count === 0
                ? "全単語"
                : `頻出 ${count.toLocaleString()} 語を習得済み`}
            </option>
          ))}
        </select>
      </label>
      <Slider
        value={size}
        onChange={(value) => setSize(value as number)}
        onChangeEnd={(value) =>
          void save({ ...settings, size: value as number }).catch(() => {})
        }
        minValue={40}
        maxValue={150}
        step={5}
        isDisabled={saving}
        className="flex max-w-sm flex-col gap-2"
      >
        <div className="flex justify-between text-sm">
          <Label className="text-ink-muted">ルビのサイズ</Label>
          <SliderOutput>{size}%</SliderOutput>
        </div>
        <SliderTrack className="relative h-6">
          <div className="absolute top-1/2 h-1 w-full -translate-y-1/2 rounded bg-white/20" />
          <SliderThumb className="top-1/2 h-4 w-4 rounded-full bg-accent outline-none data-[focus-visible]:ring-2 data-[focus-visible]:ring-white" />
        </SliderTrack>
      </Slider>
      <p className="text-xs leading-relaxed text-ink-muted">
        出現頻度を目安に、習得済みの範囲より難しい単語に英和辞書の訳を表示します。設定は字幕バーとスタジオに反映され、次回起動にも引き継がれます。
      </p>
      <p className="text-xs text-ink-muted">
        色の目安: <span className="text-emerald-300">頻出 3,000 語以内</span>・
        <span className="text-purple-300">5,000 語以内</span>・
        <span className="text-amber-300">それ以外</span>
      </p>
      {error && (
        <p role="alert" className="text-xs text-amber-300">
          保存できませんでした: {error}
          設定を選び直してください。
        </p>
      )}
    </section>
  );
}
