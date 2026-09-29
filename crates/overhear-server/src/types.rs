//! GraphQL のスキーマ型。core のドメイン型を射影する。

use async_graphql::{Enum, ID, SimpleObject};
use overhear_core::dict as core_dict;
use overhear_core::model as core;
use overhear_core::pipeline as core_pipeline;
use overhear_core::vocab as core_vocab;

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
    /// 聞き直しの再生中は入力を無音として扱っている。
    pub muted: bool,
    /// two-pass ASR の後段が有効か。
    pub refiner: Option<String>,
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

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(remote = "core_dict::Pos")]
pub enum Pos {
    Noun,
    Verb,
    Adjective,
    Adverb,
}

#[derive(SimpleObject, Clone)]
pub struct DictSense {
    pub definition: String,
    /// 同じ synset に属する語。
    pub synonyms: Vec<String>,
    pub examples: Vec<String>,
}

#[derive(SimpleObject, Clone)]
pub struct DictEntry {
    /// 活用を解いた見出し語。
    pub lemma: String,
    pub pos: Pos,
    pub pos_label: String,
    pub senses: Vec<DictSense>,
    pub source: String,
}

#[derive(SimpleObject, Clone)]
pub struct VocabItem {
    pub id: ID,
    pub lemma: String,
    pub surface: String,
    /// 保存した時点の文。元の segment が消えても読める。
    pub sentence: String,
    pub translation: Option<String>,
    pub definition: Option<String>,
    /// 焼き付けた音声があるか。実体は Anki 書き出し時に使う。
    pub has_audio: bool,
    pub created_at: String,
    pub anki_note_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct AnkiExportFailure {
    pub vocab_id: ID,
    pub reason: String,
}

#[derive(SimpleObject, Clone)]
pub struct AnkiExportResult {
    pub exported: Vec<ID>,
    pub failures: Vec<AnkiExportFailure>,
}

impl From<core_dict::DictSense> for DictSense {
    fn from(s: core_dict::DictSense) -> Self {
        Self {
            definition: s.definition,
            synonyms: s.synonyms,
            examples: s.examples,
        }
    }
}

impl From<core_dict::DictEntry> for DictEntry {
    fn from(e: core_dict::DictEntry) -> Self {
        Self {
            pos_label: e.pos.label().to_string(),
            lemma: e.lemma,
            pos: e.pos.into(),
            senses: e.senses.into_iter().map(DictSense::from).collect(),
            source: e.source,
        }
    }
}

impl From<core_vocab::VocabItem> for VocabItem {
    fn from(v: core_vocab::VocabItem) -> Self {
        Self {
            id: ID(v.id.to_string()),
            lemma: v.lemma,
            surface: v.surface,
            sentence: v.sentence,
            translation: v.translation,
            definition: v.definition,
            has_audio: v.audio_path.is_some(),
            created_at: v.created_at,
            anki_note_id: v.anki_note_id.map(|n| n.to_string()),
        }
    }
}

impl From<core_pipeline::AnkiExportResult> for AnkiExportResult {
    fn from(r: core_pipeline::AnkiExportResult) -> Self {
        Self {
            exported: r
                .exported
                .into_iter()
                .map(|id| ID(id.to_string()))
                .collect(),
            failures: r
                .failures
                .into_iter()
                .map(|f| AnkiExportFailure {
                    vocab_id: ID(f.vocab_id.to_string()),
                    reason: f.reason,
                })
                .collect(),
        }
    }
}
