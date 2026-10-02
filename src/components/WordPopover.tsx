/**
 * 選択した単語の辞書。
 *
 * 字幕と履歴の単語ボタンを起点にし、開いている語だけ辞書を引く。
 * 高さの小さい字幕ウィンドウでは、全面のダイアログに表示する。
 */
import { useQuery } from "@apollo/client/react";
import { useState, useSyncExternalStore, type RefObject } from "react";
import {
  Button,
  Dialog,
  Heading,
  Modal,
  ModalOverlay,
  Popover,
} from "react-aria-components";

import { LOOKUP } from "../lib/queries";
import { displayText } from "../lib/casing";
import type { DictEntry, Segment, Token } from "../lib/types";

type Props = {
  token: Token;
  /** 表示する語。大文字・小文字を整えたもの。辞書は `token.surface` で引く。 */
  label: string;
  segment: Segment;
  triggerRef: RefObject<Element | null>;
  dialogId: string;
  onClose: () => void;
  onSave?: (segment: Segment, token: Token) => Promise<void>;
  saving?: boolean;
};

export function WordPopover({
  token,
  label,
  segment,
  triggerRef,
  dialogId,
  onClose,
  onSave,
  saving,
}: Props) {
  const compact = useSyncExternalStore(subscribeCompact, isCompact);
  const { data, loading, error, refetch } = useQuery<{ lookup: DictEntry[] }>(
    LOOKUP,
    {
      variables: { word: token.surface },
    },
  );
  const [saveMessage, setSaveMessage] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const entries = data?.lookup ?? [];

  const save = async () => {
    setSaveMessage(null);
    try {
      await onSave?.(segment, token);
      setSaved(true);
      setSaveMessage(`「${label}」を語彙に保存しました`);
    } catch (err) {
      setSaveMessage(err instanceof Error ? err.message : String(err));
    }
  };

  const dialog = (
    <Dialog
      id={dialogId}
      aria-label={`${label} の辞書`}
      className="dictionary-dialog flex min-h-0 flex-col outline-none"
    >
      <div className="flex shrink-0 items-center justify-between gap-3 border-b border-white/10 px-4 py-2">
        <Heading
          slot="title"
          className="min-w-0 break-words text-lg font-semibold"
        >
          {label}
        </Heading>
        <div className="flex shrink-0 items-center gap-2">
          {onSave && (
            <Button
              isDisabled={saving || saved}
              className="rounded bg-accent/20 px-2 py-1 text-xs outline-none data-[disabled]:opacity-50 data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-accent/30"
              onPress={() => void save()}
            >
              {saved ? "保存済み" : saving ? "保存中…" : "語彙に保存"}
            </Button>
          )}
          <Button
            onPress={onClose}
            aria-label="辞書を閉じる"
            className="rounded px-2 py-1 text-xs text-ink-muted outline-none data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-white/10"
          >
            閉じる
          </Button>
        </div>
      </div>
      <div
        role="region"
        aria-label="語義"
        tabIndex={0}
        className="min-h-0 overflow-y-auto overscroll-contain px-4 py-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent"
      >
        {saveMessage && (
          <p role="status" className="mb-3 text-accent">
            {saveMessage}
          </p>
        )}
        {loading ? (
          <p role="status" className="text-ink-muted">
            辞書を引いている…
          </p>
        ) : error ? (
          <div role="alert">
            <p className="text-amber-300">辞書を取得できませんでした。</p>
            <Button
              className="mt-2 rounded bg-white/10 px-2 py-1 outline-none data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent"
              onPress={() => void refetch().catch(() => {})}
            >
              再試行
            </Button>
          </div>
        ) : entries.length === 0 ? (
          <p className="text-ink-muted">
            「{label}」は辞書に見つかりませんでした。
          </p>
        ) : (
          <div className="flex flex-col gap-4">
            {entries.map((entry, index) => (
              <section key={`${entry.source}-${entry.lemma}-${index}`}>
                <h3 className="flex flex-wrap items-baseline gap-2">
                  <span className="font-medium">{entry.lemma}</span>
                  {entry.posLabel && (
                    <span className="rounded bg-white/10 px-1 text-xs text-ink-muted">
                      {entry.posLabel}
                    </span>
                  )}
                  <span className="text-xs text-ink-muted">
                    {entry.source === "ejdict"
                      ? "英和 · ejdict"
                      : entry.source === "wordnet"
                        ? "英英 · WordNet"
                        : entry.source}
                  </span>
                </h3>
                {entry.ipaUs && (
                  <p className="mt-1 break-words" aria-label="米語の発音記号">
                    <span className="mr-2 text-xs text-ink-muted">米 IPA</span>
                    <span lang="en-US">{entry.ipaUs}</span>
                  </p>
                )}
                <ol className="mt-1 list-decimal space-y-2 pl-5 text-ink-muted">
                  {entry.senses.map((sense, i) => (
                    <li key={i}>
                      {sense.definition}
                      {sense.synonyms.length > 0 && (
                        <span className="ml-1 opacity-70">
                          ＝ {sense.synonyms.join(", ")}
                        </span>
                      )}
                      {sense.examples.map((example, j) => (
                        <p key={j} className="mt-1 text-xs italic">
                          {example}
                        </p>
                      ))}
                    </li>
                  ))}
                </ol>
              </section>
            ))}
          </div>
        )}
        {!compact && (
          <p className="mt-4 border-t border-white/10 pt-3 text-xs text-ink-muted">
            {displayText(segment)}
          </p>
        )}
      </div>
    </Dialog>
  );

  return compact ? (
    <ModalOverlay
      isOpen
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      isDismissable
      className="fixed inset-0 z-50 flex bg-surface-raised"
    >
      <Modal className="flex h-full w-full min-w-0 flex-col">{dialog}</Modal>
    </ModalOverlay>
  ) : (
    <Popover
      isOpen
      triggerRef={triggerRef}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      placement="bottom"
      containerPadding={8}
      maxHeight={480}
      className="dictionary-popover z-50 flex w-96 max-w-[calc(100vw-16px)] flex-col overflow-hidden rounded-lg border border-white/10 bg-surface-raised shadow-xl"
    >
      {dialog}
    </Popover>
  );
}

const compactQuery = window.matchMedia("(max-height: 320px)");

function isCompact() {
  return compactQuery.matches;
}

function subscribeCompact(onChange: () => void) {
  compactQuery.addEventListener("change", onChange);
  return () => compactQuery.removeEventListener("change", onChange);
}
