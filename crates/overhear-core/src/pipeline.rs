//! キャプチャ → ASR → segment 化 → 翻訳 の配線。
//!
//! ASR の Partial / Final を「同じ id の segment の更新」として表現するのが
//! 肝で、two-pass ASR の差し替えもフロント側では通常の更新として扱える。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::sync::{broadcast, mpsc};

use crate::asr::{AsrEvent, AsrToken, Recognizer};
use crate::audio::{self, CaptureConfig, CaptureHandle};
use crate::model::{Segment, SegmentId, SegmentStatus, Token};
use crate::ring::RingBuffer;
use crate::translate::{TranslateRequest, TranslatorRegistry};

/// Final の末尾に足す余白。発話末が切れた音声を書き出さないため。
const TAIL_MARGIN_MS: u64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineChoice {
    /// april-asr (既定)。低遅延・英語。
    April,
    /// モデル無しで動かす擬似エンジン。
    Mock,
}

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub sample_rate: u32,
    pub ring_seconds: usize,
    pub target_lang: String,
    pub history_limit: usize,
    pub engine: EngineChoice,
    pub capture: CaptureConfig,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16_000,
            ring_seconds: 600, // 10 分
            target_lang: "ja".to_string(),
            history_limit: 500,
            engine: EngineChoice::April,
            capture: CaptureConfig::default(),
        }
    }
}

pub struct Overhear {
    pub config: RuntimeConfig,
    pub ring: Arc<Mutex<RingBuffer>>,
    pub segments: Arc<RwLock<VecDeque<Segment>>>,
    pub updates: broadcast::Sender<Segment>,
    pub translators: Arc<TranslatorRegistry>,
    /// この時刻までは入力を無音として扱う。聞き直しの再生音を
    /// 自分の monitor から拾い直さないための窓。
    mute_until: Arc<Mutex<Option<Instant>>>,
    _capture: CaptureHandle,
}

impl Overhear {
    /// キャプチャと ASR を起動する。tokio のランタイム上で呼ぶこと。
    pub fn start(config: RuntimeConfig, translators: Arc<TranslatorRegistry>) -> Result<Arc<Self>> {
        let (audio_tx, mut audio_rx) = mpsc::unbounded_channel::<Vec<i16>>();
        let (asr_tx, mut asr_rx) = mpsc::unbounded_channel::<AsrEvent>();

        let capture = audio::spawn(&config.capture, audio_tx).context("音声キャプチャの起動")?;

        let mut recognizer: Box<dyn Recognizer> = match config.engine {
            #[cfg(feature = "april")]
            EngineChoice::April => Box::new(crate::asr::april::AprilRecognizer::from_env(
                asr_tx.clone(),
            )?),
            #[cfg(not(feature = "april"))]
            EngineChoice::April => {
                anyhow::bail!("april フィーチャが無効なビルドです")
            }
            EngineChoice::Mock => Box::new(crate::asr::mock::MockRecognizer::new(
                config.sample_rate,
                asr_tx.clone(),
            )),
        };
        let asr_engine = recognizer.id();

        let ring = Arc::new(Mutex::new(RingBuffer::new(
            config.sample_rate,
            config.ring_seconds,
        )));
        let segments = Arc::new(RwLock::new(VecDeque::<Segment>::new()));
        let (updates, _) = broadcast::channel::<Segment>(256);

        let mute_until: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));

        // 音声を ring に溜めつつ ASR へ供給する。ASR 側はブロッキング API
        // なので専用スレッドに置く。
        {
            let ring = Arc::clone(&ring);
            let mute_until = Arc::clone(&mute_until);
            tokio::task::spawn_blocking(move || {
                while let Some(chunk) = audio_rx.blocking_recv() {
                    let muted = mute_until
                        .lock()
                        .ok()
                        .and_then(|g| *g)
                        .is_some_and(|until| Instant::now() < until);

                    // ミュート中は破棄せず無音に差し替える。破棄すると ASR に
                    // 供給した累積時間が止まり、リングバッファの絶対時間軸と
                    // ずれてしまうため。
                    let data = if muted {
                        vec![0i16; chunk.len()]
                    } else {
                        chunk
                    };

                    if let Ok(mut ring) = ring.lock() {
                        ring.push(&data);
                    }
                    if let Err(err) = recognizer.feed(&data) {
                        tracing::warn!(?err, "ASR への供給に失敗した");
                        break;
                    }
                }
                let _ = recognizer.flush();
                tracing::info!("ASR スレッドを終了した");
            });
        }

        let this = Arc::new(Self {
            config: config.clone(),
            ring,
            segments: Arc::clone(&segments),
            updates: updates.clone(),
            translators,
            mute_until,
            _capture: capture,
        });

        // ASR イベントを segment に畳む。
        {
            let this = Arc::clone(&this);
            tokio::spawn(async move {
                let mut builder = SegmentBuilder::new(asr_engine);
                while let Some(event) = asr_rx.recv().await {
                    if let Some(segment) = builder.apply(event) {
                        this.publish(segment);
                    }
                }
                tracing::info!("ASR イベントの購読を終了した");
            });
        }

        Ok(this)
    }

    /// segment を履歴へ反映し、購読者へ流す。Final なら翻訳を起動する。
    fn publish(self: &Arc<Self>, segment: Segment) {
        let is_final = segment.status == SegmentStatus::Final;
        self.upsert(segment.clone());
        let _ = self.updates.send(segment.clone());

        if is_final && !segment.source_text.is_empty() {
            let this = Arc::clone(self);
            tokio::spawn(async move {
                this.translate_segment(segment.id, None).await;
            });
        }
    }

    fn upsert(&self, segment: Segment) {
        let Ok(mut segments) = self.segments.write() else {
            return;
        };
        if let Some(existing) = segments.iter_mut().find(|s| s.id == segment.id) {
            *existing = segment;
        } else {
            segments.push_back(segment);
            while segments.len() > self.config.history_limit {
                segments.pop_front();
            }
        }
    }

    /// 指定時間だけ入力を無音として扱う。
    ///
    /// 既定シンクへ鳴らした音は自分の monitor に戻ってくるので、聞き直しの
    /// 間これを閉じておかないと、再生した音声がもう一度書き起こされて
    /// 履歴が汚れる。
    pub fn mute_for(&self, ms: u64) {
        if let Ok(mut guard) = self.mute_until.lock() {
            *guard = Some(Instant::now() + Duration::from_millis(ms));
        }
    }

    pub fn is_muted(&self) -> bool {
        self.mute_until
            .lock()
            .ok()
            .and_then(|g| *g)
            .is_some_and(|until| Instant::now() < until)
    }

    pub fn segment(&self, id: SegmentId) -> Option<Segment> {
        self.segments
            .read()
            .ok()?
            .iter()
            .find(|s| s.id == id)
            .cloned()
    }

    pub fn recent(&self, limit: usize) -> Vec<Segment> {
        let Ok(segments) = self.segments.read() else {
            return Vec::new();
        };
        segments.iter().rev().take(limit).rev().cloned().collect()
    }

    /// segment の音声をリングバッファから切り出す。溢れていれば None。
    pub fn segment_audio(&self, id: SegmentId) -> Option<Vec<i16>> {
        let segment = self.segment(id)?;
        let ring = self.ring.lock().ok()?;
        ring.slice_ms(segment.start_ms, segment.end_ms)
    }

    /// 指定エンジンで翻訳し、結果を segment に追加して再配信する。
    pub async fn translate_segment(
        self: &Arc<Self>,
        id: SegmentId,
        engine_id: Option<&str>,
    ) -> Option<Segment> {
        let segment = self.segment(id)?;
        let req = TranslateRequest {
            text: segment.source_text.clone(),
            source_lang: None,
            target_lang: self.config.target_lang.clone(),
        };
        let translation = self.translators.translate(engine_id, &req).await?;
        if translation.text.is_empty() {
            return Some(segment);
        }

        let mut updated = self.segment(id)?;
        updated.translations.push(translation);
        self.upsert(updated.clone());
        let _ = self.updates.send(updated.clone());
        Some(updated)
    }
}

/// ASR イベントを segment に畳む状態機械。
struct SegmentBuilder {
    next_id: SegmentId,
    current: Option<Segment>,
    asr_engine: &'static str,
}

impl SegmentBuilder {
    fn new(asr_engine: &'static str) -> Self {
        Self {
            next_id: 1,
            current: None,
            asr_engine,
        }
    }

    fn apply(&mut self, event: AsrEvent) -> Option<Segment> {
        match event {
            AsrEvent::Partial { text, tokens } => {
                if text.is_empty() {
                    return None;
                }
                let segment = self.build(text, &tokens, SegmentStatus::Interim);
                self.current = Some(segment.clone());
                Some(segment)
            }
            AsrEvent::Final { text, tokens } => {
                if text.is_empty() {
                    self.current = None;
                    return None;
                }
                let segment = self.build(text, &tokens, SegmentStatus::Final);
                // 次の Partial は新しい id で始まる。
                self.current = None;
                self.next_id += 1;
                Some(segment)
            }
            AsrEvent::Silence => None,
            AsrEvent::CantKeepUp => {
                tracing::warn!("ASR が実時間に追いつけていない");
                None
            }
        }
    }

    fn build(&self, text: String, tokens: &[AsrToken], status: SegmentStatus) -> Segment {
        let start_ms = tokens.first().map(|t| t.time_ms).unwrap_or(0);
        let last_ms = tokens.last().map(|t| t.time_ms).unwrap_or(start_ms);
        let end_ms = if status == SegmentStatus::Final {
            last_ms + TAIL_MARGIN_MS
        } else {
            last_ms
        };

        Segment {
            id: self.next_id,
            start_ms,
            end_ms,
            status,
            source_text: text,
            tokens: merge_words(tokens),
            translations: self
                .current
                .as_ref()
                .filter(|c| c.id == self.next_id)
                .map(|c| c.translations.clone())
                .unwrap_or_default(),
            asr_engine: self.asr_engine.to_string(),
        }
    }
}

/// ASR のトークンを語にまとめる。
///
/// april-asr はサブワード単位で返す (`T` `RA` `N` `S` …) ため、そのままでは
/// 単語クリックの辞書引きに使えない。語境界フラグ (または先頭の空白) で
/// 区切り直して 1 語 1 トークンにする。
fn merge_words(tokens: &[AsrToken]) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::new();
    for t in tokens {
        let starts_word = t.word_boundary || t.raw.starts_with(' ');
        if starts_word || out.is_empty() {
            out.push(Token {
                index: 0, // 後で振り直す
                surface: t.surface.clone(),
                start_ms: t.time_ms,
                word_boundary: true,
                sentence_end: t.sentence_end,
            });
        } else if let Some(last) = out.last_mut() {
            last.surface.push_str(&t.surface);
            last.sentence_end |= t.sentence_end;
        }
    }
    // 空白だけのトークンが語頭として来るため、中身の無い語を落としてから採番する。
    out.retain(|t| !t.surface.is_empty());
    for (index, token) in out.iter_mut().enumerate() {
        token.index = index;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(raw: &str, boundary: bool, time_ms: u64) -> AsrToken {
        AsrToken {
            raw: raw.to_string(),
            surface: raw.trim().to_string(),
            logprob: -0.1,
            word_boundary: boundary,
            sentence_end: false,
            time_ms,
        }
    }

    #[test]
    fn subword_tokens_are_merged_into_words() {
        // april-asr は " T" "RA" "N" "S" のようにサブワードで返す。
        let tokens = vec![
            tok(" WELL", true, 1680),
            tok(",", false, 1760),
            tok(" ", true, 2480),
            tok("T", false, 2480),
            tok("RA", false, 2520),
            tok("N", false, 2560),
            tok("S", false, 2640),
        ];
        let words = merge_words(&tokens);
        let surfaces: Vec<&str> = words.iter().map(|t| t.surface.as_str()).collect();
        assert_eq!(surfaces, vec!["WELL,", "TRANS"]);
        assert_eq!(words[0].start_ms, 1680);
        assert_eq!(words[1].start_ms, 2480);
        assert_eq!(words[1].index, 1);
    }
}
