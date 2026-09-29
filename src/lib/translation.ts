/**
 * 台詞の翻訳の依頼。
 *
 * 翻訳は自動では掛からない (台詞が途切れたときの最新行を除く)。
 * 依頼した segment を覚えておき、訳が届くまで「翻訳中」と出す。
 * 訳そのものは segmentUpdates で届き、正規化キャッシュが反映する。
 */
import { useMutation } from "@apollo/client/react";
import { useCallback, useState } from "react";

import { REQUEST_TRANSLATION } from "./queries";
import type { Segment } from "./types";

export function useTranslationRequests() {
  const [requested, setRequested] = useState<ReadonlySet<string>>(new Set());
  const [requestTranslation] = useMutation(REQUEST_TRANSLATION);

  const request = useCallback(
    (segment: Segment) => {
      setRequested((prev) => new Set(prev).add(segment.id));
      void requestTranslation({ variables: { segmentId: segment.id } });
    },
    [requestTranslation],
  );

  const isPending = useCallback(
    (segment: Segment) =>
      requested.has(segment.id) && segment.translations.length === 0,
    [requested],
  );

  return { request, isPending };
}
