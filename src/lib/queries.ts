import { gql } from "@apollo/client";

export const SEGMENT_FIELDS = gql`
  fragment SegmentFields on Segment {
    id
    startMs
    endMs
    status
    sourceText
    asrEngine
    audioUrl
    tokens {
      index
      surface
      startMs
      sentenceEnd
    }
    translations {
      engineId
      text
      targetLang
      fallbackFrom
    }
  }
`;

export const SEGMENTS = gql`
  ${SEGMENT_FIELDS}
  query Segments($limit: Int) {
    segments(limit: $limit) {
      ...SegmentFields
    }
  }
`;

/** 新規 segment と差し替えの両方がここに流れる。 */
export const SEGMENT_UPDATES = gql`
  ${SEGMENT_FIELDS}
  subscription SegmentUpdates {
    segmentUpdates {
      ...SegmentFields
    }
  }
`;

export const CAPTURE_STATE = gql`
  query CaptureState {
    captureState {
      running
      sampleRate
      ringSeconds
      capturedMs
      asrEngine
      targetLang
      muted
      refiner
      captureTarget
      translationBacklog
    }
  }
`;

export const TRANSLATION_ENGINES = gql`
  query TranslationEngines {
    translationEngines {
      id
      displayName
      available
      unavailableReason
      sendsDataExternally
      isDefault
    }
  }
`;

/** 自動翻訳に使うエンジンを切り替える。サーバが次の起動にも持ち越す。 */
export const SET_DEFAULT_TRANSLATOR = gql`
  mutation SetDefaultTranslator($engineId: ID!) {
    setDefaultTranslator(engineId: $engineId) {
      id
      isDefault
    }
  }
`;

export const RETRANSLATE = gql`
  ${SEGMENT_FIELDS}
  mutation Retranslate($segmentId: ID!, $engineId: ID) {
    retranslate(segmentId: $segmentId, engineId: $engineId) {
      ...SegmentFields
    }
  }
`;

/** 聞き直しの再生音を拾い直さないよう、再生の間だけ入力を閉じる。 */
export const MUTE_CAPTURE = gql`
  mutation MuteCapture($ms: Int!) {
    muteCapture(ms: $ms) {
      muted
    }
  }
`;

export const LOOKUP = gql`
  query Lookup($word: String!) {
    lookup(word: $word) {
      lemma
      posLabel
      source
      senses {
        definition
        synonyms
        examples
      }
    }
  }
`;

export const VOCAB_FIELDS = gql`
  fragment VocabFields on VocabItem {
    id
    lemma
    surface
    sentence
    translation
    definition
    hasAudio
    createdAt
    ankiNoteId
  }
`;

export const VOCAB = gql`
  ${VOCAB_FIELDS}
  query Vocab($limit: Int) {
    vocab(limit: $limit) {
      ...VocabFields
    }
  }
`;

export const SAVE_VOCAB = gql`
  ${VOCAB_FIELDS}
  mutation SaveVocab($segmentId: ID!, $tokenIndex: Int!) {
    saveVocab(segmentId: $segmentId, tokenIndex: $tokenIndex) {
      ...VocabFields
    }
  }
`;

export const REMOVE_VOCAB = gql`
  mutation RemoveVocab($id: ID!) {
    removeVocab(id: $id)
  }
`;

export const EXPORT_TO_ANKI = gql`
  mutation ExportToAnki($vocabIds: [ID!]!, $deck: String) {
    exportToAnki(vocabIds: $vocabIds, deck: $deck) {
      exported
      failures {
        vocabId
        reason
      }
    }
  }
`;

export const AUDIO_DEVICES = gql`
  query AudioDevices {
    audioDevices {
      id
      description
      kind
      isDefault
    }
  }
`;

/** 拾う先を切り替える。deviceId を省くと既定シンクに戻る。 */
export const SET_CAPTURE_DEVICE = gql`
  mutation SetCaptureDevice($deviceId: ID) {
    setCaptureDevice(deviceId: $deviceId) {
      captureTarget
    }
  }
`;
