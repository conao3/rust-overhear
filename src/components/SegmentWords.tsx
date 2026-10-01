import { useId, useRef, useState } from "react";
import { Button } from "react-aria-components";

import { displayText, displayWords } from "../lib/casing";
import type { Segment, Token } from "../lib/types";
import { WordPopover } from "./WordPopover";

type Props = {
  segment: Segment;
  onSaveWord?: (segment: Segment, token: Token) => Promise<void>;
  savingWord?: boolean;
};

export function SegmentWords({ segment, onSaveWord, savingWord }: Props) {
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
      {displayed.tokens.map((token, i) => (
        <span key={token.index}>
          <Button
            className="rounded px-0.5 outline-none transition-colors data-[focus-visible]:ring-2 data-[focus-visible]:ring-accent data-[hovered]:bg-accent/25 data-[pressed]:bg-accent/40"
            aria-label={`${words[i]} を調べる`}
            aria-haspopup="dialog"
            aria-expanded={selected?.token.index === token.index}
            aria-controls={
              selected?.token.index === token.index ? dialogId : undefined
            }
            onPress={(event) => {
              triggerRef.current = event.target;
              setSelected({ segment: displayed, token, label: words[i] });
            }}
          >
            {words[i]}
          </Button>{" "}
        </span>
      ))}
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
}
