import { memo, useId, useRef, useState } from "react";

import { useRuby } from "../lib/ruby";
import { displayText, displayWords } from "../lib/casing";
import type { Segment, Token } from "../lib/types";
import { WordPopover } from "./WordPopover";

type Props = {
  segment: Segment;
  onSaveWord?: (segment: Segment, token: Token) => Promise<void>;
  savingWord?: boolean;
};

export const SegmentWords = memo(function SegmentWords({
  segment,
  onSaveWord,
  savingWord,
}: Props) {
  const { settings } = useRuby();
  const [selected, setSelected] = useState<{
    segment: Segment;
    token: Token;
    label: string;
  } | null>(null);
  const triggerRef = useRef<Element | null>(null);
  const dialogId = useId();
  const displayed = selected?.segment ?? segment;
  const words = displayWords(displayed);

  if (displayed.tokens.length === 0) return displayText(displayed);

  return (
    <>
      <span
        className="segment-words"
        data-ruby-enabled={settings?.enabled ?? true}
      >
        {displayed.tokens.map((token, i) => {
          const ruby =
            settings?.enabled &&
            token.ruby &&
            token.ruby.rank > settings.knownWords
              ? token.ruby
              : null;
          return (
            <button
              key={token.index}
              type="button"
              data-ruby-level={
                ruby
                  ? ruby.rank <= 3000
                    ? "common"
                    : ruby.rank <= 5000
                      ? "intermediate"
                      : "advanced"
                  : undefined
              }
              className="rounded px-0.5 outline-none transition-colors focus-visible:ring-2 focus-visible:ring-accent hover:bg-accent/25 active:bg-accent/40"
              aria-label={`${words[i]} を調べる`}
              aria-haspopup="dialog"
              aria-expanded={selected?.token.index === token.index}
              aria-controls={
                selected?.token.index === token.index ? dialogId : undefined
              }
              onClick={(event) => {
                triggerRef.current = event.currentTarget;
                setSelected({ segment: displayed, token, label: words[i] });
              }}
            >
              {(settings?.enabled ?? true) ? (
                <ruby>
                  <span className="word-surface">{words[i]}</span>
                  <rt aria-hidden="true" title={ruby?.text}>
                    {ruby?.text}
                  </rt>
                </ruby>
              ) : (
                words[i]
              )}
            </button>
          );
        })}
      </span>
      {selected && (
        <WordPopover
          token={selected.token}
          label={selected.label}
          segment={selected.segment}
          triggerRef={triggerRef}
          dialogId={dialogId}
          onClose={() => setSelected(null)}
          onSave={onSaveWord}
          saving={savingWord}
        />
      )}
    </>
  );
});
