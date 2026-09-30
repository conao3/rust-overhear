export type SegmentStatus = "INTERIM" | "FINAL";

export type Token = {
  index: number;
  surface: string;
  startMs: number;
  sentenceEnd: boolean;
};

export type Translation = {
  engineId: string;
  text: string;
  targetLang: string;
  fallbackFrom: string | null;
};

export type Segment = {
  id: string;
  startMs: number;
  endMs: number;
  status: SegmentStatus;
  sourceText: string;
  asrEngine: string;
  audioUrl: string;
  tokens: Token[];
  translations: Translation[];
  /** 直近の翻訳が失敗した理由。訳が付けば消える。 */
  translationError: string | null;
};

export type TranslationEngineInfo = {
  id: string;
  displayName: string;
  available: boolean;
  unavailableReason: string | null;
  sendsDataExternally: boolean;
  isDefault: boolean;
};

export type CaptureState = {
  running: boolean;
  sampleRate: number;
  ringSeconds: number;
  capturedMs: number;
  asrEngine: string;
  targetLang: string;
  muted: boolean;
  refiner: string | null;
  captureTarget: string | null;
  translationBacklog: number;
};

export type AudioDevice = {
  id: string;
  description: string;
  kind: "SINK" | "SOURCE";
  isDefault: boolean;
};

export type DictSense = {
  definition: string;
  synonyms: string[];
  examples: string[];
};

export type DictEntry = {
  lemma: string;
  /** 品詞。辞書によっては持たない (英和など)。 */
  posLabel: string | null;
  source: string;
  senses: DictSense[];
};

export type VocabItem = {
  id: string;
  lemma: string;
  surface: string;
  sentence: string;
  translation: string | null;
  definition: string | null;
  hasAudio: boolean;
  createdAt: string;
  ankiNoteId: string | null;
};

export type AnkiExportResult = {
  exported: string[];
  failures: { vocabId: string; reason: string }[];
};

export type Preferences = {
  targetLang: string;
  ollamaModel: string;
  /** pull 済みの Ollama モデル。Ollama に繋がらなければ空。 */
  ollamaModels: string[];
  apiKeys: { engineId: string; configured: boolean }[];
};
