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
};
