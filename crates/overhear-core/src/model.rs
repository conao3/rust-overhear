//! パイプラインを流れるドメイン型。GraphQL 層はこれを射影する。

use serde::{Deserialize, Serialize};

pub type SegmentId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SegmentStatus {
    /// ASR の途中経過。同じ id で後から差し替わる。
    Interim,
    /// 確定。以後この id のテキストは変わらない。
    Final,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub index: usize,
    /// 表層形。april-asr のトークンは先頭の空白で語境界を表すため、
    /// ここでは trim した見た目の語を入れる。
    pub surface: String,
    pub start_ms: u64,
    pub word_boundary: bool,
    pub sentence_end: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Translation {
    pub engine_id: String,
    pub text: String,
    pub target_lang: String,
    /// 指定エンジンが失敗して代替した場合、元のエンジン id。
    pub fallback_from: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub id: SegmentId,
    pub start_ms: u64,
    pub end_ms: u64,
    pub status: SegmentStatus,
    pub source_text: String,
    pub tokens: Vec<Token>,
    /// エンジンごとに 0..n 件。retranslate は上書きではなく追加。
    pub translations: Vec<Translation>,
    /// 直近の翻訳が失敗した理由。訳が付けば消える。
    pub translation_error: Option<String>,
    pub asr_engine: String,
}

impl Segment {
    pub fn duration_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}
