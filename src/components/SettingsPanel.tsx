/**
 * 設定タブ。訳す先の言語、Ollama のモデル、外部翻訳の API キー。
 *
 * 値はサーバが settings.json と Secret Service に残すので、ここは
 * 変えた値を送って返ってきた状態を出すだけ。API キーは送るだけで、
 * サーバからは「入っているか」しか返らない。
 */
import { useMutation, useQuery } from "@apollo/client/react";
import { useEffect, useState } from "react";
import {
  Button,
  Input,
  Label,
  ListBox,
  ListBoxItem,
  Popover,
  Select,
  SelectValue,
  TextField,
  Switch,
} from "react-aria-components";

import {
  PREFERENCES,
  CAPTURE_STATE,
  SET_REFINEMENT_ENABLED,
  SET_API_KEY,
  SET_OLLAMA_MODEL,
  SET_TARGET_LANG,
  TRANSLATION_ENGINES,
} from "../lib/queries";
import { RubyControls } from "./RubyControls";
import type { Preferences } from "../lib/types";

const LANGUAGES = [
  { id: "ja", label: "日本語" },
  { id: "en", label: "英語" },
  { id: "zh", label: "中国語" },
  { id: "ko", label: "韓国語" },
  { id: "es", label: "スペイン語" },
  { id: "fr", label: "フランス語" },
  { id: "de", label: "ドイツ語" },
];

const ENGINE_NAMES: Record<string, string> = {
  deepl: "DeepL",
  google: "Google Cloud Translation",
};

const buttonClass =
  "rounded bg-white/10 px-2 py-1 text-sm outline-none data-[disabled]:opacity-50 data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-white/15";
const itemClass =
  "cursor-pointer rounded px-2 py-1.5 text-sm outline-none data-[focused]:bg-accent/25";

type Choice = { id: string; label: string };

function Picker({
  label,
  choices,
  selected,
  onChange,
}: {
  label: string;
  choices: Choice[];
  selected: string;
  onChange: (id: string) => void;
}) {
  return (
    <Select
      selectedKey={selected}
      onSelectionChange={(key) => onChange(String(key))}
      className="flex items-center gap-3"
    >
      <Label className="w-32 text-sm text-ink-muted">{label}</Label>
      <Button className={buttonClass}>
        <SelectValue />
      </Button>
      <Popover className="max-h-80 overflow-y-auto rounded-lg border border-white/10 bg-surface-raised p-1 shadow-xl">
        <ListBox className="outline-none" items={choices}>
          {(choice) => (
            <ListBoxItem
              id={choice.id}
              textValue={choice.label}
              className={itemClass}
            >
              {choice.label}
            </ListBoxItem>
          )}
        </ListBox>
      </Popover>
    </Select>
  );
}

function ApiKeyRow({
  engineId,
  configured,
  onSave,
  saving,
}: {
  engineId: string;
  configured: boolean;
  onSave: (engineId: string, key: string | null) => Promise<void>;
  saving: boolean;
}) {
  const [draft, setDraft] = useState("");
  return (
    <div className="flex items-center gap-3">
      <TextField
        type="password"
        value={draft}
        onChange={setDraft}
        className="flex items-center gap-3"
      >
        <Label className="w-32 text-sm text-ink-muted">
          {ENGINE_NAMES[engineId] ?? engineId}
        </Label>
        <Input
          placeholder={configured ? "設定済み (入れ直すと上書き)" : "API キー"}
          className="w-72 rounded bg-white/5 px-2 py-1 text-sm outline-none ring-1 ring-white/10 data-[focused]:ring-accent"
        />
      </TextField>
      <Button
        className={buttonClass}
        isDisabled={saving || draft.trim() === ""}
        onPress={() => void onSave(engineId, draft).then(() => setDraft(""))}
      >
        保存
      </Button>
      {configured && (
        <Button
          className={buttonClass}
          isDisabled={saving}
          onPress={() => void onSave(engineId, null)}
        >
          消す
        </Button>
      )}
      <span
        className={
          configured ? "text-xs text-emerald-300" : "text-xs text-ink-muted/70"
        }
      >
        {configured ? "設定済み" : "未設定"}
      </span>
    </div>
  );
}

export function SettingsPanel() {
  const { data, startPolling, stopPolling } = useQuery<{
    preferences: Preferences;
  }>(PREFERENCES);
  const [
    setRefinementEnabled,
    { loading: savingRefinement, error: refinementMutationError },
  ] = useMutation(SET_REFINEMENT_ENABLED, {
    refetchQueries: [PREFERENCES, CAPTURE_STATE],
    awaitRefetchQueries: true,
  });
  // 変えたら設定とエンジンの利用可否を取り直す。
  const refresh = { refetchQueries: [PREFERENCES, TRANSLATION_ENGINES] };
  const [setTargetLang] = useMutation(SET_TARGET_LANG, refresh);
  const [setOllamaModel] = useMutation(SET_OLLAMA_MODEL, refresh);
  const [setApiKey, { loading: savingKey }] = useMutation(SET_API_KEY, refresh);
  const [message, setMessage] = useState<string | null>(null);

  const prefs = data?.preferences;
  const startingRefinement =
    prefs?.refinementEnabled &&
    !prefs.refinementReady &&
    !prefs.refinementError;
  useEffect(() => {
    if (startingRefinement) startPolling(1000);
    else stopPolling();
    return stopPolling;
  }, [startingRefinement, startPolling, stopPolling]);
  if (!prefs) {
    return <p className="text-sm text-ink-muted">読み込み中…</p>;
  }

  const models: Choice[] = prefs.ollamaModels.map((m) => ({ id: m, label: m }));
  if (!models.some((m) => m.id === prefs.ollamaModel)) {
    models.unshift({
      id: prefs.ollamaModel,
      label: `${prefs.ollamaModel} (pull されていない)`,
    });
  }

  const saveKey = async (engineId: string, key: string | null) => {
    setMessage(null);
    try {
      await setApiKey({ variables: { engineId, key } });
      setMessage(
        key === null
          ? `${ENGINE_NAMES[engineId]} のキーを消した`
          : `${ENGINE_NAMES[engineId]} のキーを保存した`,
      );
    } catch (err) {
      setMessage(
        `保存できなかった (${err instanceof Error ? err.message : String(err)})`,
      );
    }
  };

  return (
    <div className="flex flex-col gap-6">
      <RubyControls />
      <section className="flex flex-col gap-3">
        <h2 className="text-sm font-semibold">音声認識</h2>
        <Switch
          isSelected={prefs.refinementEnabled}
          isDisabled={savingRefinement || !prefs.refinementAvailable}
          onChange={(enabled) =>
            void setRefinementEnabled({ variables: { enabled } }).catch(
              () => {},
            )
          }
          aria-describedby="refinement-description"
          className="group flex w-fit cursor-pointer items-center gap-3 text-sm outline-none data-[disabled]:cursor-default data-[disabled]:opacity-50"
        >
          <span className="flex h-5 w-9 shrink-0 items-center rounded-full bg-white/20 p-0.5 group-data-[selected]:bg-accent group-data-[focus-visible]:ring-2 group-data-[focus-visible]:ring-accent group-data-[focus-visible]:ring-offset-2 group-data-[focus-visible]:ring-offset-surface">
            <span className="h-4 w-4 rounded-full bg-white group-data-[selected]:translate-x-4" />
          </span>
          字幕の精度補正
          <span className="text-xs text-ink-muted">
            {prefs.refinementEnabled ? "オン" : "オフ"}
          </span>
        </Switch>
        <p
          id="refinement-description"
          className="text-xs leading-relaxed text-ink-muted"
        >
          発話後に音声をもう一度解析して、字幕の単語や句読点を補正します。
          オンにするとCPU使用量が増えます。オフでも即時字幕・辞書・聞き直しは使えます。
          設定は次回の起動にも引き継がれます。
        </p>
        {startingRefinement && (
          <p role="status" className="text-xs text-ink-muted">
            補正用のモデルを読み込み中…
          </p>
        )}
        {!prefs.refinementAvailable && (
          <p className="text-xs text-ink-muted">
            この起動設定では精度補正を利用できません。
          </p>
        )}
        {(refinementMutationError || prefs.refinementError) && (
          <p role="alert" className="text-xs text-amber-300">
            精度補正を変更・実行できませんでした:{" "}
            {refinementMutationError?.message ?? prefs.refinementError}
          </p>
        )}
      </section>
      <section className="flex flex-col gap-3">
        <h2 className="text-sm font-semibold">翻訳</h2>
        <Picker
          label="訳す先の言語"
          choices={LANGUAGES}
          selected={prefs.targetLang}
          onChange={(lang) => void setTargetLang({ variables: { lang } })}
        />
        <Picker
          label="Ollama のモデル"
          choices={models}
          selected={prefs.ollamaModel}
          onChange={(model) => void setOllamaModel({ variables: { model } })}
        />
        {prefs.ollamaModels.length === 0 && (
          <p className="text-xs text-ink-muted">
            Ollama に繋がらない。`ollama serve` を起動し、モデルを pull する
          </p>
        )}
      </section>

      <section className="flex flex-col gap-3">
        <h2 className="text-sm font-semibold">外部翻訳の API キー</h2>
        <p className="text-xs text-ink-muted">
          キーは Secret Service (gnome-keyring 等)
          に保存する。これらのエンジンは字幕の本文を外部へ送る。
        </p>
        {prefs.apiKeys.map((k) => (
          <ApiKeyRow
            key={k.engineId}
            engineId={k.engineId}
            configured={k.configured}
            onSave={saveKey}
            saving={savingKey}
          />
        ))}
        {message && <p className="text-sm text-ink-muted">{message}</p>}
      </section>
    </div>
  );
}
