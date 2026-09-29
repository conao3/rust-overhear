//! 音声認識エンジンの抽象。
//!
//! 翻訳と同じくストラテジーパターンにしてある。two-pass ASR
//! (april-asr で即時 → whisper.cpp で確定文に差し替え) は複数エンジンを
//! 前提とするため、呼び出し側はどの実装が動いているかを知らない。

use anyhow::Result;

#[cfg(feature = "april")]
pub mod april;
pub mod mock;
pub mod whisper;

#[derive(Debug, Clone)]
pub struct AsrToken {
    /// エンジンが返した生のトークン。april-asr は語頭を先頭の空白で表す。
    pub raw: String,
    /// 表示・辞書引き用に trim した語。
    pub surface: String,
    pub logprob: f32,
    pub word_boundary: bool,
    pub sentence_end: bool,
    /// その語が話された位置。セッションに供給した音声の累積 ms で表す。
    pub time_ms: u64,
}

#[derive(Debug, Clone)]
pub enum AsrEvent {
    /// 途中経過。後続の Partial / Final が同じ発話を上書きする。
    Partial { text: String, tokens: Vec<AsrToken> },
    /// 確定。次の Partial は新しい発話として始まる。
    Final { text: String, tokens: Vec<AsrToken> },
    /// 無音を検出した。
    Silence,
    /// 処理が実時間に追いつかない。
    CantKeepUp,
}

pub trait Recognizer: Send {
    fn id(&self) -> &'static str;
    fn sample_rate(&self) -> u32;
    /// PCM16 mono を供給する。イベントは生成時に渡したチャネルへ流れる。
    fn feed(&mut self, pcm: &[i16]) -> Result<()>;
    /// 未処理の音声を処理して確定結果を出させる。
    fn flush(&mut self) -> Result<()>;
}

/// トークン列から表示用のテキストを組み立てる。
pub fn tokens_to_text(tokens: &[AsrToken]) -> String {
    let joined: String = tokens.iter().map(|t| t.raw.as_str()).collect();
    joined.trim().to_string()
}
