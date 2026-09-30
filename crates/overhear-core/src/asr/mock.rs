//! モデル無しでパイプライン全体を動かすための擬似エンジン。
//!
//! フロントエンドの開発と CI で使う。供給された音声量に応じて、
//! 用意した文を 1 語ずつ Partial として伸ばし、文末で Final を出す。

use anyhow::Result;
use tokio::sync::mpsc::UnboundedSender;

use super::{AsrEvent, AsrToken, Fed, Recognizer, tokens_to_text};

const SENTENCES: &[&str] = &[
    "the quick brown fox jumps over the lazy dog",
    "she sells seashells by the seashore",
    "how much wood would a woodchuck chuck",
];

/// 1 語を出すのに要する音声の長さ。
const MS_PER_WORD: u64 = 400;

pub struct MockRecognizer {
    tx: UnboundedSender<AsrEvent>,
    sample_rate: u32,
    fed_samples: u64,
    /// 現在の文の index。
    sentence: usize,
    /// 現在の文で出し終えた語数。
    emitted: usize,
    /// 現在の文を開始した時点の累積 ms。
    sentence_start_ms: u64,
}

impl MockRecognizer {
    pub fn new(sample_rate: u32, tx: UnboundedSender<AsrEvent>) -> Self {
        Self {
            tx,
            sample_rate,
            fed_samples: 0,
            sentence: 0,
            emitted: 0,
            sentence_start_ms: 0,
        }
    }

    fn now_ms(&self) -> u64 {
        self.fed_samples * 1000 / self.sample_rate as u64
    }

    fn tokens_for(&self, words: &[&str]) -> Vec<AsrToken> {
        words
            .iter()
            .enumerate()
            .map(|(i, w)| AsrToken {
                raw: format!(" {w}"),
                surface: w.to_string(),
                logprob: -0.1,
                word_boundary: true,
                sentence_end: false,
                time_ms: self.sentence_start_ms + i as u64 * MS_PER_WORD,
            })
            .collect()
    }
}

impl Recognizer for MockRecognizer {
    fn id(&self) -> &'static str {
        "mock"
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn feed(&mut self, pcm: &[i16]) -> Result<Fed> {
        self.fed_samples += pcm.len() as u64;
        let words: Vec<&str> = SENTENCES[self.sentence].split(' ').collect();
        let elapsed = self.now_ms().saturating_sub(self.sentence_start_ms);
        let want = ((elapsed / MS_PER_WORD) as usize).min(words.len());

        while self.emitted < want {
            self.emitted += 1;
            let tokens = self.tokens_for(&words[..self.emitted]);
            let text = tokens_to_text(&tokens);
            let _ = self.tx.send(AsrEvent::Partial { text, tokens });
        }

        if self.emitted == words.len() && elapsed >= words.len() as u64 * MS_PER_WORD {
            let tokens = self.tokens_for(&words);
            let text = tokens_to_text(&tokens);
            let _ = self.tx.send(AsrEvent::Final { text, tokens });
            self.sentence = (self.sentence + 1) % SENTENCES.len();
            self.emitted = 0;
            self.sentence_start_ms = self.now_ms();
        }
        Ok(Fed::Accepted)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}
