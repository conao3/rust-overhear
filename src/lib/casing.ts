/**
 * april の大文字だけの出力を、読みやすい大文字・小文字に整える (表示だけ)。
 *
 * april の英語モデルは全部大文字で返す。whisper で差し替わる前の行と、
 * whisper に掛けない短い行・古い行がこれに当たる。小文字にして、文頭と
 * 一人称の I だけを大文字にする (Live Captions の token_capitalizer と同じ規則)。
 * 固有名詞は小文字のままになる。辞書引きは元の語で行う。
 */
import type { Segment } from "./types";

/** 英字を含み、小文字が 1 つも無い。 */
export function isAllCaps(text: string): boolean {
  return /[A-Z]/.test(text) && !/[a-z]/.test(text);
}

const SENTENCE_END = /[.!?]["')\]]*$/;

/** 最初の英字を大文字にする。 */
function capitalize(word: string): string {
  const i = word.search(/[A-Za-z]/);
  return i < 0
    ? word
    : word.slice(0, i) + word[i].toUpperCase() + word.slice(i + 1);
}

/** 語の並びを整える。`sentenceEnd` は語が文末か。 */
export function caseWords(
  words: { surface: string; sentenceEnd: boolean }[],
): string[] {
  let atSentenceStart = true;
  return words.map(({ surface, sentenceEnd }) => {
    let word = surface.toLowerCase();
    // I / I'M / I'LL / I'D / I'VE
    word = word.replace(/^i(?=$|')/, "I");
    if (atSentenceStart) word = capitalize(word);
    atSentenceStart = sentenceEnd || SENTENCE_END.test(surface);
    return word;
  });
}

/** 行の語ごとの表示。大文字だけの行でなければ手を加えない。 */
export function displayWords(segment: Segment): string[] {
  if (!isAllCaps(segment.sourceText)) {
    return segment.tokens.map((t) => t.surface);
  }
  return caseWords(segment.tokens);
}

/** 行の表示。トークンが無い行は本文を語に分けて整える。 */
export function displayText(segment: Segment): string {
  if (!isAllCaps(segment.sourceText)) return segment.sourceText;
  const words =
    segment.tokens.length > 0
      ? segment.tokens
      : segment.sourceText
          .split(/\s+/)
          .filter(Boolean)
          .map((surface) => ({ surface, sentenceEnd: false }));
  return caseWords(words).join(" ");
}
