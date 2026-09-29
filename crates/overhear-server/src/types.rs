//! GraphQL のスキーマ型。core のドメイン型を射影する。

use async_graphql::{Enum, ID, SimpleObject};
use overhear_core::model as core;

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(remote = "core::SegmentStatus")]
pub enum SegmentStatus {
    /// ASR の途中経過。同じ id で後から差し替わる。
    Interim,
    /// 確定。以後この id のテキストは変わらない。
    Final,
}

#[derive(SimpleObject, Clone)]
pub struct Token {
    pub index: i32,
    pub surface: String,
    pub start_ms: i32,
    pub word_boundary: bool,
    pub sentence_end: bool,
}

#[derive(SimpleObject, Clone)]
pub struct Translation {
    pub engine_id: ID,
    pub text: String,
    pub target_lang: String,
    /// 指定エンジンが失敗して代替した場合の元エンジン。
    pub fallback_from: Option<ID>,
}

#[derive(SimpleObject, Clone)]
pub struct Segment {
    pub id: ID,
    pub start_ms: i32,
    pub end_ms: i32,
    pub status: SegmentStatus,
    pub source_text: String,
    pub tokens: Vec<Token>,
    /// エンジンごとに 0..n 件。retranslate は上書きではなく追加。
    pub translations: Vec<Translation>,
    pub asr_engine: String,
    /// 音声の実体。GraphQL にバイナリは載せない。
    pub audio_url: String,
}

#[derive(SimpleObject, Clone)]
pub struct TranslationEngineInfo {
    pub id: ID,
    pub display_name: String,
    pub available: bool,
    pub unavailable_reason: Option<String>,
    pub sends_data_externally: bool,
    pub supported_target_langs: Vec<String>,
    pub is_default: bool,
}

#[derive(SimpleObject, Clone)]
pub struct CaptureState {
    pub running: bool,
    pub sample_rate: i32,
    pub ring_seconds: i32,
    pub captured_ms: i32,
    pub asr_engine: String,
    pub target_lang: String,
}

impl From<core::Token> for Token {
    fn from(t: core::Token) -> Self {
        Self {
            index: t.index as i32,
            surface: t.surface,
            start_ms: t.start_ms as i32,
            word_boundary: t.word_boundary,
            sentence_end: t.sentence_end,
        }
    }
}

impl From<core::Translation> for Translation {
    fn from(t: core::Translation) -> Self {
        Self {
            engine_id: ID(t.engine_id),
            text: t.text,
            target_lang: t.target_lang,
            fallback_from: t.fallback_from.map(ID),
        }
    }
}

impl From<core::Segment> for Segment {
    fn from(s: core::Segment) -> Self {
        Self {
            audio_url: format!("/audio/{}.wav", s.id),
            id: ID(s.id.to_string()),
            start_ms: s.start_ms as i32,
            end_ms: s.end_ms as i32,
            status: s.status.into(),
            source_text: s.source_text,
            tokens: s.tokens.into_iter().map(Token::from).collect(),
            translations: s.translations.into_iter().map(Translation::from).collect(),
            asr_engine: s.asr_engine,
        }
    }
}
