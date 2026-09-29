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

export const RETRANSLATE = gql`
  ${SEGMENT_FIELDS}
  mutation Retranslate($segmentId: ID!, $engineId: ID) {
    retranslate(segmentId: $segmentId, engineId: $engineId) {
      ...SegmentFields
    }
  }
`;
