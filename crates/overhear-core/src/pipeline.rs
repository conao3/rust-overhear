//! キャプチャ → ASR → segment 化 → 翻訳 の配線。
//!
//! ASR の Partial / Final を「同じ id の segment の更新」として表現するのが
//! 肝で、two-pass ASR の差し替えもフロント側では通常の更新として扱える。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::sync::{broadcast, mpsc};

use crate::anki::{AnkiConnect, AnkiNote, DEFAULT_DECK, DEFAULT_MODEL};
use crate::asr::whisper::WhisperRefiner;
use crate::asr::{AsrEvent, AsrToken, Recognizer};
use crate::audio::{self, CaptureConfig, CaptureHandle};
use crate::devices::{self, AudioDevice, DeviceKind};
use crate::dict::{DictEntry, DictionaryRegistry, normalize_surface};
use crate::gate::{ClockMap, SilenceGate};
use crate::model::{Segment, SegmentId, SegmentStatus, Token};
use crate::ring::RingBuffer;
use crate::settings::SettingsStore;
use crate::translate::{TranslateRequest, TranslatorRegistry};
use crate::vocab::{NewVocab, VocabItem, VocabStore};

/// Final の末尾に足す余白。発話末が切れた音声を書き出さないため。
const TAIL_MARGIN_MS: u64 = 500;

/// これより短い区間は whisper に掛けない。
/// 相槌や物音の誤検出が大半で、CPU を使うわりに得るものが無い。
const MIN_REFINE_MS: u64 = 600;

/// キャプチャが止まってから起動し直すまでの待ち。失敗が続くと倍々に延ばす。
const CAPTURE_RETRY_MIN: Duration = Duration::from_secs(1);
const CAPTURE_RETRY_MAX: Duration = Duration::from_secs(30);

/// 台詞がこれだけ途切れたら、最新の台詞を訳す。
/// 動画を止めたときに、いま画面にある台詞の訳が出る。
const PAUSE_TRANSLATE_MS: u64 = 1500;

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
    /// 無音とみなす振幅のしきい値。0 でゲートを無効にする。
    pub silence_threshold: u16,
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
            silence_threshold: crate::gate::DEFAULT_THRESHOLD,
        }
    }
}

/// パイプラインが使う外部サービス一式。
///
/// 翻訳・辞書・語彙ストア・Anki はいずれも差し替え可能な境界なので、
/// まとめて渡して `Overhear` 本体の引数が増えないようにしてある。
pub struct Services {
    pub translators: Arc<TranslatorRegistry>,
    /// two-pass ASR の後段。無ければ april の出力をそのまま確定とする。
    pub whisper: Option<Arc<WhisperRefiner>>,
    pub dictionaries: Arc<DictionaryRegistry>,
    pub vocab: Arc<VocabStore>,
    pub anki: Arc<AnkiConnect>,
    pub settings: Arc<SettingsStore>,
    pub api_keys: Arc<crate::secrets::ApiKeys>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AnkiExportFailure {
    pub vocab_id: i64,
    pub reason: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AnkiExportResult {
    pub exported: Vec<i64>,
    pub failures: Vec<AnkiExportFailure>,
}

pub struct Overhear {
    pub config: RuntimeConfig,
    pub ring: Arc<Mutex<RingBuffer>>,
    pub segments: Arc<RwLock<VecDeque<Segment>>>,
    pub updates: broadcast::Sender<Segment>,
    pub translators: Arc<TranslatorRegistry>,
    pub whisper: Option<Arc<WhisperRefiner>>,
    pub dictionaries: Arc<DictionaryRegistry>,
    pub vocab: Arc<VocabStore>,
    pub anki: Arc<AnkiConnect>,
    settings: Arc<SettingsStore>,
    api_keys: Arc<crate::secrets::ApiKeys>,
    /// 訳す先の言語。設定から実行中に切り替えられる。
    target_lang: RwLock<String>,
    /// whisper での差し替え待ち。1 本の worker が新しいものから処理する。
    refine_queue: WorkQueue,
    /// 翻訳待ち。1 本の worker が新しいものから訳す。
    translate_queue: WorkQueue,
    /// whisper の worker が segment を処理している最中か。
    refining: AtomicBool,
    /// 最後に ASR が segment を出した時刻。台詞の途切れを測る。
    last_speech: Mutex<Instant>,
    /// 途切れで最後に訳へ回した segment。同じ台詞を二度積まない。
    last_pause_translated: AtomicU64,
    /// この時刻までは入力を無音として扱う。聞き直しの再生音を
    /// 自分の monitor から拾い直さないための窓。
    mute_until: Arc<Mutex<Option<Instant>>>,
    /// 差し替えられるよう、キャプチャの子プロセスと供給先を持っておく。
    capture: Mutex<Option<CaptureHandle>>,
    capture_config: Mutex<CaptureConfig>,
    audio_tx: mpsc::UnboundedSender<Vec<i16>>,
}

impl Overhear {
    /// キャプチャと ASR を起動する。tokio のランタイム上で呼ぶこと。
    pub fn start(mut config: RuntimeConfig, services: Services) -> Result<Arc<Self>> {
        // 前回選んだ音源に戻す。抜かれた USB 機器のように見つからなければ
        // 既定シンクで起動する (起動できないよりよい)。
        if let Some(saved) = services.settings.get().capture_target {
            match resolve_capture(&config.capture, Some(&saved)) {
                Ok(capture) => config.capture = capture,
                Err(err) => tracing::warn!(%err, "保存した音源が無いので既定シンクで起動する"),
            }
        }

        let (audio_tx, mut audio_rx) = mpsc::unbounded_channel::<Vec<i16>>();
        let (asr_tx, mut asr_rx) = mpsc::unbounded_channel::<AsrEvent>();

        let capture =
            audio::spawn(&config.capture, audio_tx.clone()).context("音声キャプチャの起動")?;

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
        // ASR へ供給しなかったぶん ASR の内部時計は遅れる。どこでどれだけ
        // 飛ばしたかを残し、segment を組むときにトークンごとに足し戻す。
        let clock = Arc::new(Mutex::new(ClockMap::default()));

        // 音声を ring に溜めつつ ASR へ供給する。ASR 側はブロッキング API
        // なので専用スレッドに置く。
        {
            let ring = Arc::clone(&ring);
            let mute_until = Arc::clone(&mute_until);
            let clock = Arc::clone(&clock);
            let mut gate =
                SilenceGate::new(config.silence_threshold, crate::gate::DEFAULT_HANGOVER);
            let gate_enabled = config.silence_threshold > 0;

            tokio::task::spawn_blocking(move || {
                let mut fed: u64 = 0;
                while let Some(chunk) = audio_rx.blocking_recv() {
                    // リングバッファには常に入れる。聞き直しは無音区間も
                    // 含めて成立している必要がある。
                    if let Ok(mut ring) = ring.lock() {
                        ring.push(&chunk);
                    }

                    let muted = mute_until
                        .lock()
                        .ok()
                        .and_then(|g| *g)
                        .is_some_and(|until| Instant::now() < until);

                    if !gate_enabled {
                        // ゲート無効時も、ミュート中は無音を送って時計を進める。
                        let data = if muted {
                            vec![0i16; chunk.len()]
                        } else {
                            chunk
                        };
                        if let Err(err) = recognizer.feed(&data) {
                            tracing::warn!(?err, "ASR への供給に失敗した");
                            break;
                        }
                        continue;
                    }

                    // 静かな区間とミュート中は ASR を動かさない。ここが
                    // 待機時の CPU をほぼゼロにする。
                    let mut failed = false;
                    let pieces = gate.admit(&chunk, muted);
                    // この admit で流すものは、ここまでの飛ばし量の時間軸に乗る。
                    if let Ok(mut clock) = clock.lock() {
                        clock.record(fed, gate.skipped_samples());
                    }
                    for piece in pieces {
                        if let Err(err) = recognizer.feed(&piece) {
                            tracing::warn!(?err, "ASR への供給に失敗した");
                            failed = true;
                            break;
                        }
                        fed += piece.len() as u64;
                    }
                    if failed {
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
            translators: services.translators,
            whisper: services.whisper,
            dictionaries: services.dictionaries,
            vocab: services.vocab,
            anki: services.anki,
            settings: services.settings,
            api_keys: services.api_keys,
            target_lang: RwLock::new(config.target_lang.clone()),
            refine_queue: WorkQueue::default(),
            translate_queue: WorkQueue::default(),
            refining: AtomicBool::new(false),
            last_speech: Mutex::new(Instant::now()),
            last_pause_translated: AtomicU64::new(0),
            mute_until,
            capture: Mutex::new(Some(capture)),
            capture_config: Mutex::new(config.capture.clone()),
            audio_tx,
        });

        if this.whisper.is_some() {
            let this = Arc::clone(&this);
            tokio::spawn(async move { this.run_refine_worker().await });
        }
        {
            let this = Arc::clone(&this);
            tokio::spawn(async move { this.run_translation_worker().await });
        }
        {
            let this = Arc::clone(&this);
            tokio::spawn(async move { this.run_pause_watcher().await });
        }
        {
            let this = Arc::clone(&this);
            tokio::spawn(async move { this.run_capture_supervisor().await });
        }

        // ASR イベントを segment に畳む。
        {
            let this = Arc::clone(&this);
            let sample_rate = config.sample_rate;
            tokio::spawn(async move {
                let mut builder = SegmentBuilder::new(asr_engine);
                while let Some(event) = asr_rx.recv().await {
                    // ASR の時計は供給を止めたぶん遅れている。リングバッファと
                    // 同じ絶対時間に戻してから segment にする。
                    let segment = {
                        let Ok(clock) = clock.lock() else { break };
                        builder.apply(event, |ms| clock.to_absolute_ms(ms, sample_rate))
                    };
                    if let Some(segment) = segment {
                        this.publish(segment);
                    }
                }
                tracing::info!("ASR イベントの購読を終了した");
            });
        }

        Ok(this)
    }

    /// segment を履歴へ反映し、購読者へ流す。Final なら whisper の差し替えに積む。
    ///
    /// 翻訳は自動では掛けない。台詞が続く動画では CPU 推論の翻訳が追いつかず、
    /// 訳が出る前に次の台詞へ進んでしまう。訳すのは求められたとき
    /// ([`Self::request_translation`]) と、台詞が途切れたとき (最新の 1 行) だけ。
    fn publish(self: &Arc<Self>, segment: Segment) {
        let is_final = segment.status == SegmentStatus::Final;
        if let Ok(mut last) = self.last_speech.lock() {
            *last = Instant::now();
        }
        self.upsert(segment.clone());
        let _ = self.updates.send(segment.clone());

        if is_final && !segment.source_text.is_empty() && self.whisper.is_some() {
            self.refine_queue.push(segment.id);
        }
    }

    /// segment の翻訳を待ち行列に積む。結果は subscription で届く。
    pub fn request_translation(&self, id: SegmentId) {
        self.translate_queue.push(id);
    }

    /// 翻訳待ちの件数。
    pub fn translation_backlog(&self) -> usize {
        self.translate_queue.len()
    }

    /// whisper の差し替えを 1 本ずつ、新しい segment から順に流す。
    ///
    /// 古い順に処理すると、溜まった分を捌き終えるまで新しい文が差し替えも
    /// 翻訳もされない。翻訳と同じく、いま表示している文を先に処理する。
    async fn run_refine_worker(self: Arc<Self>) {
        loop {
            let id = self.refine_queue.pop().await;
            self.refining.store(true, Ordering::Relaxed);
            self.refine_segment(id).await;
            self.refining.store(false, Ordering::Relaxed);
        }
    }

    /// キャプチャが動いているか。
    pub fn is_capturing(&self) -> bool {
        self.capture
            .lock()
            .ok()
            .is_some_and(|slot| slot.as_ref().is_some_and(|c| c.is_alive()))
    }

    /// pw-record が終了したら起動し直す。
    ///
    /// 既定のデバイスを抜く、PipeWire が再起動する、といったことで pw-record は
    /// 終了する。放っておくと字幕が黙って止まる。
    async fn run_capture_supervisor(self: Arc<Self>) {
        let mut tick = tokio::time::interval(CAPTURE_RETRY_MIN);
        let mut wait = CAPTURE_RETRY_MIN;
        let mut next_attempt = Instant::now();
        loop {
            tick.tick().await;
            if self.is_capturing() {
                wait = CAPTURE_RETRY_MIN;
                continue;
            }
            if Instant::now() < next_attempt {
                continue;
            }
            let config = match self.capture_config.lock() {
                Ok(config) => config.clone(),
                Err(_) => return,
            };
            match audio::spawn(&config, self.audio_tx.clone()) {
                Ok(capture) => {
                    tracing::info!(target = ?config.target, "音声キャプチャを起動し直した");
                    if let Ok(mut slot) = self.capture.lock() {
                        if let Some(old) = slot.replace(capture) {
                            old.stop();
                        }
                    }
                }
                Err(err) => tracing::warn!(%err, "音声キャプチャを起動し直せなかった"),
            }
            // 起動直後に落ちる (デバイスが無い等) ときに詰めて再試行しない。
            next_attempt = Instant::now() + wait;
            wait = (wait * 2).min(CAPTURE_RETRY_MAX);
        }
    }

    /// 台詞が途切れたら、最新の台詞を 1 行だけ訳へ回す。
    ///
    /// whisper の差し替えが済むのを待ってから積む。翻訳の入力を句読点つきの
    /// 読める文にするため。
    async fn run_pause_watcher(self: Arc<Self>) {
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        loop {
            tick.tick().await;
            let paused = self
                .last_speech
                .lock()
                .is_ok_and(|last| last.elapsed() >= Duration::from_millis(PAUSE_TRANSLATE_MS));
            let refined = self.refine_queue.len() == 0 && !self.refining.load(Ordering::Relaxed);
            if !paused || !refined {
                continue;
            }
            let latest = self
                .segments
                .read()
                .ok()
                .and_then(|segments| segments.back().cloned());
            let Some(latest) = latest else { continue };
            if latest.status != SegmentStatus::Final
                || !latest.translations.is_empty()
                || self.last_pause_translated.load(Ordering::Relaxed) == latest.id
            {
                continue;
            }
            self.last_pause_translated
                .store(latest.id, Ordering::Relaxed);
            self.translate_queue.push(latest.id);
        }
    }

    /// 翻訳を 1 本ずつ、新しい segment から順に流す。
    ///
    /// ローカル LLM の翻訳は 1 行に数秒かかる。続けて何行も求められたときは、
    /// いま表示している文を先に訳す。
    async fn run_translation_worker(self: Arc<Self>) {
        loop {
            let id = self.translate_queue.pop().await;
            // 履歴から溢れたもの、引き直しで既に訳があるもの、
            // `[BLANK_AUDIO]` のような台詞の無い区間は飛ばす。
            let needs_translation = self.segment(id).is_some_and(|s| {
                s.translations.is_empty() && !crate::asr::whisper::is_non_speech(&s.source_text)
            });
            if needs_translation {
                self.translate_segment(id, None).await;
            }
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

    /// 音声の入出力ノードを列挙する。
    pub fn audio_devices(&self) -> Vec<AudioDevice> {
        devices::list().unwrap_or_else(|err| {
            tracing::warn!(%err, "音声デバイスを列挙できなかった");
            Vec::new()
        })
    }

    /// 現在キャプチャしているノード。None なら既定シンク。
    pub fn capture_target(&self) -> Option<String> {
        self.capture_config
            .lock()
            .ok()
            .and_then(|c| c.target.clone())
    }

    /// 拾う先を切り替える。`None` で既定シンクに戻す。
    ///
    /// pw-record の子プロセスだけを差し替え、ASR とリングバッファは
    /// そのまま使い続ける。時間軸は受け取ったサンプル数で進むので、
    /// 切り替えで生じる空白のぶん進まないだけで整合は崩れない。
    pub fn set_capture_device(&self, device_id: Option<&str>) -> Result<()> {
        let mut config = self
            .capture_config
            .lock()
            .map_err(|_| anyhow::anyhow!("lock poisoned"))?;
        *config = resolve_capture(&config, device_id)?;

        let new_capture =
            audio::spawn(&config, self.audio_tx.clone()).context("音声キャプチャの切り替え")?;
        if let Ok(mut slot) = self.capture.lock() {
            // 先に新しいものを立ててから古いものを止める。
            if let Some(old) = slot.replace(new_capture) {
                old.stop();
            }
        }
        tracing::info!(target = ?config.target, "キャプチャ先を切り替えた");
        let target = config.target.clone();
        self.settings.update(|s| s.capture_target = target)
    }

    pub fn target_lang(&self) -> String {
        self.target_lang
            .read()
            .map(|l| l.clone())
            .unwrap_or_default()
    }

    /// 訳す先の言語を切り替え、次の起動にも持ち越す。
    pub fn set_target_lang(&self, lang: &str) -> Result<()> {
        let lang = lang.trim();
        if lang.is_empty() {
            anyhow::bail!("言語が空");
        }
        *self
            .target_lang
            .write()
            .map_err(|_| anyhow::anyhow!("lock poisoned"))? = lang.to_string();
        self.settings
            .update(|s| s.target_lang = Some(lang.to_string()))
    }

    /// Ollama のモデルを切り替え、次の起動にも持ち越す。
    pub fn set_ollama_model(&self, model: &str) -> Result<()> {
        self.translators.ollama().set_model(model)?;
        self.settings
            .update(|s| s.ollama_model = Some(model.to_string()))
    }

    /// API キーを Secret Service に保存する。`None` で消す。
    pub fn set_api_key(&self, engine_id: &str, key: Option<&str>) -> Result<()> {
        self.api_keys.set(engine_id, key)
    }

    pub fn has_api_key(&self, engine_id: &str) -> bool {
        self.api_keys.get(engine_id).is_some()
    }

    /// 自動翻訳に使うエンジンを切り替え、次の起動にも持ち越す。
    pub fn set_default_translator(&self, id: &str) -> Result<()> {
        self.translators.set_default(id)?;
        self.settings
            .update(|s| s.translator = Some(id.to_string()))
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

    /// 確定した segment を whisper.cpp の出力で差し替える (two-pass の後段)。
    ///
    /// april は低遅延だが全部大文字で句読点が無い。文が閉じた後に
    /// リングバッファの該当区間を whisper へ投げ、同じ id の更新として
    /// 配信し直す。
    async fn refine_segment(self: &Arc<Self>, id: SegmentId) -> Option<Segment> {
        let whisper = self.whisper.as_ref()?;
        let segment = self.segment(id)?;
        if segment.asr_engine.contains("whisper") {
            return None; // 二重に掛けない
        }
        if segment.duration_ms() < MIN_REFINE_MS {
            return None; // 短すぎる区間は掛けるだけ無駄
        }

        let (start_ms, end_ms) = self.refine_span(&segment);
        let audio = {
            let ring = self.ring.lock().ok()?;
            ring.slice_ms(start_ms, end_ms)?
        };
        let text = match whisper.refine(&audio, self.config.sample_rate).await {
            Ok(text) if !text.is_empty() => text,
            Ok(_) => return None,
            Err(err) => {
                // 後段が落ちても april の出力は残る。
                tracing::warn!(%err, "whisper での差し替えに失敗した");
                return None;
            }
        };

        let mut updated = self.segment(id)?;
        updated.tokens = words_from_text(&text, updated.start_ms, updated.end_ms);
        updated.source_text = text;
        updated.asr_engine = format!("{}+whisper", updated.asr_engine);
        self.upsert(updated.clone());
        let _ = self.updates.send(updated.clone());
        Some(updated)
    }

    /// whisper に渡す区間。前後に余白を取り、隣の segment に食い込む分は削る。
    ///
    /// 台詞が詰まっていると、末尾の余白が次の台詞の頭を拾って whisper の文に
    /// 混ざる (`... the magic. See you!`)。
    fn refine_span(&self, segment: &Segment) -> (u64, u64) {
        let mut start = WhisperRefiner::lead_in(segment.start_ms);
        let mut end = segment.end_ms;
        if let Ok(segments) = self.segments.read() {
            if let Some(prev) = segments.iter().rev().find(|s| s.id < segment.id) {
                start = start.max(prev.end_ms.saturating_sub(TAIL_MARGIN_MS));
            }
            if let Some(next) = segments.iter().find(|s| s.id > segment.id) {
                end = end.min(next.start_ms);
            }
        }
        (start.min(segment.start_ms), end.max(segment.start_ms + 1))
    }

    /// 単語を辞書で引く。
    pub fn lookup(&self, word: &str) -> Vec<DictEntry> {
        self.dictionaries.lookup(word)
    }

    /// segment の 1 語を語彙ストアへ保存する。
    ///
    /// 保存時点の文・訳・語義・音声を焼き付けるので、後から segment が
    /// 履歴から溢れても、リングバッファから音声が消えても残る。
    pub fn save_vocab(&self, segment_id: SegmentId, token_index: usize) -> Result<VocabItem> {
        let segment = self
            .segment(segment_id)
            .ok_or_else(|| anyhow::anyhow!("segment {segment_id} が見つからない"))?;
        let token = segment
            .tokens
            .get(token_index)
            .ok_or_else(|| anyhow::anyhow!("token {token_index} が見つからない"))?;

        let entries = self.dictionaries.lookup(&token.surface);
        let lemma = entries
            .first()
            .map(|e| e.lemma.clone())
            .unwrap_or_else(|| normalize_surface(&token.surface));
        let definition = entries
            .first()
            .and_then(|e| e.senses.first())
            .map(|s| s.definition.clone());

        self.vocab.save(NewVocab {
            lemma,
            surface: token.surface.clone(),
            sentence: segment.source_text.clone(),
            translation: segment.translations.last().map(|t| t.text.clone()),
            definition,
            audio: self.segment_audio(segment_id),
            sample_rate: self.config.sample_rate,
        })
    }

    /// 語彙を Anki へ送る。1 件ずつ失敗理由を返す。
    pub async fn export_to_anki(&self, ids: &[i64], deck: Option<&str>) -> AnkiExportResult {
        let deck = deck.unwrap_or(DEFAULT_DECK);
        let mut result = AnkiExportResult {
            exported: Vec::new(),
            failures: Vec::new(),
        };

        if let Err(err) = self.anki.ensure_deck(deck).await {
            for id in ids {
                result.failures.push(AnkiExportFailure {
                    vocab_id: *id,
                    reason: err.to_string(),
                });
            }
            return result;
        }

        for id in ids {
            let item = match self.vocab.get(*id) {
                Ok(Some(item)) => item,
                Ok(None) => {
                    result.failures.push(AnkiExportFailure {
                        vocab_id: *id,
                        reason: "語彙が見つからない".into(),
                    });
                    continue;
                }
                Err(err) => {
                    result.failures.push(AnkiExportFailure {
                        vocab_id: *id,
                        reason: err.to_string(),
                    });
                    continue;
                }
            };

            let note = AnkiNote {
                deck: deck.to_string(),
                model: DEFAULT_MODEL.to_string(),
                front: item.lemma.clone(),
                back: build_back(&item),
                audio_path: item.audio_path.clone(),
                tags: vec!["overhear".to_string()],
            };

            match self.anki.add_note(&note).await {
                Ok(note_id) => {
                    let _ = self.vocab.mark_anki(item.id, note_id);
                    result.exported.push(item.id);
                }
                Err(err) => result.failures.push(AnkiExportFailure {
                    vocab_id: item.id,
                    reason: err.to_string(),
                }),
            }
        }
        result
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
            target_lang: self.target_lang(),
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

/// 拾う先を決める。`None` で既定シンク。
///
/// 再生側 (シンク) なら monitor を、録音側 (ソース) ならそのまま掴む。
fn resolve_capture(base: &CaptureConfig, device_id: Option<&str>) -> Result<CaptureConfig> {
    let mut config = base.clone();
    match device_id {
        Some(id) => {
            let device = devices::list()?
                .into_iter()
                .find(|d| d.id == id)
                .ok_or_else(|| anyhow::anyhow!("音声デバイスが見つからない: {id}"))?;
            config.capture_sink = matches!(device.kind, DeviceKind::Sink);
            config.target = Some(device.id);
        }
        None => {
            config.capture_sink = true;
            config.target = None;
        }
    }
    Ok(config)
}

/// 新しい segment から取り出す待ち行列。worker は空のあいだ待つ。
#[derive(Default)]
struct WorkQueue {
    pending: Mutex<NewestFirst>,
    wake: tokio::sync::Notify,
}

impl WorkQueue {
    fn push(&self, id: SegmentId) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.push(id);
        }
        self.wake.notify_one();
    }

    async fn pop(&self) -> SegmentId {
        loop {
            let next = self.pending.lock().ok().and_then(|mut p| p.pop_newest());
            if let Some(id) = next {
                return id;
            }
            self.wake.notified().await;
        }
    }

    fn len(&self) -> usize {
        self.pending.lock().map(|p| p.len()).unwrap_or(0)
    }
}

/// segment id の集合。同じ id は 1 度だけ積み、大きい (新しい) 順に出す。
#[derive(Debug, Default)]
struct NewestFirst {
    pending: Vec<SegmentId>,
}

impl NewestFirst {
    fn push(&mut self, id: SegmentId) {
        if !self.pending.contains(&id) {
            self.pending.push(id);
        }
    }

    fn pop_newest(&mut self) -> Option<SegmentId> {
        let index = self
            .pending
            .iter()
            .enumerate()
            .max_by_key(|(_, id)| **id)
            .map(|(index, _)| index)?;
        Some(self.pending.swap_remove(index))
    }

    fn len(&self) -> usize {
        self.pending.len()
    }
}

/// whisper の確定文を語トークンに割り付ける。
///
/// whisper-server は語ごとの時刻を返さないため、segment の区間に均等割りする。
/// 表示と辞書引きにはこれで足り、正確な語頭時刻が要るのは将来の機能。
fn words_from_text(text: &str, start_ms: u64, end_ms: u64) -> Vec<Token> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    let span = end_ms.saturating_sub(start_ms);
    let step = span / words.len().max(1) as u64;
    words
        .iter()
        .enumerate()
        .map(|(index, word)| Token {
            index,
            surface: (*word).to_string(),
            start_ms: start_ms + step * index as u64,
            word_boundary: true,
            sentence_end: word.ends_with(['.', '!', '?']),
        })
        .collect()
}

/// Anki の裏面。語義・原文・訳をこの順で並べる。
fn build_back(item: &VocabItem) -> String {
    let mut parts = Vec::new();
    if let Some(def) = &item.definition {
        parts.push(def.clone());
    }
    parts.push(format!("<br><br>{}", item.sentence));
    if let Some(tr) = &item.translation {
        parts.push(format!("<br>{tr}"));
    }
    parts.join("")
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

    /// `to_abs` は ASR の時刻をリングバッファの絶対時間へ直す。
    fn apply(&mut self, event: AsrEvent, to_abs: impl Fn(u64) -> u64) -> Option<Segment> {
        match event {
            AsrEvent::Partial { text, tokens } => {
                if text.is_empty() {
                    return None;
                }
                let segment = self.build(text, &tokens, SegmentStatus::Interim, &to_abs);
                self.current = Some(segment.clone());
                Some(segment)
            }
            AsrEvent::Final { text, tokens } => {
                if text.is_empty() {
                    self.current = None;
                    return None;
                }
                let segment = self.build(text, &tokens, SegmentStatus::Final, &to_abs);
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

    fn build(
        &self,
        text: String,
        tokens: &[AsrToken],
        status: SegmentStatus,
        to_abs: &impl Fn(u64) -> u64,
    ) -> Segment {
        let start_ms = to_abs(tokens.first().map(|t| t.time_ms).unwrap_or(0));
        let last_ms = tokens.last().map(|t| to_abs(t.time_ms)).unwrap_or(start_ms);
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
            tokens: merge_words(tokens, to_abs),
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
fn merge_words(tokens: &[AsrToken], to_abs: &impl Fn(u64) -> u64) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::new();
    for t in tokens {
        let starts_word = t.word_boundary || t.raw.starts_with(' ');
        if starts_word || out.is_empty() {
            out.push(Token {
                index: 0, // 後で振り直す
                surface: t.surface.clone(),
                start_ms: to_abs(t.time_ms),
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
        let words = merge_words(&tokens, &|ms| ms);
        let surfaces: Vec<&str> = words.iter().map(|t| t.surface.as_str()).collect();
        assert_eq!(surfaces, vec!["WELL,", "TRANS"]);
        assert_eq!(words[0].start_ms, 1680);
        assert_eq!(words[1].start_ms, 2480);
        assert_eq!(words[1].index, 1);
    }
}

#[cfg(test)]
mod refine_tests {
    use super::*;

    #[test]
    fn distributes_word_timings_over_the_segment() {
        let tokens = words_from_text("I have to go now.", 1000, 3000);
        assert_eq!(tokens.len(), 5);
        assert_eq!(tokens[0].surface, "I");
        assert_eq!(tokens[0].start_ms, 1000);
        // 5 語を 2000ms に均等割り → 400ms 刻み
        assert_eq!(tokens[1].start_ms, 1400);
        assert_eq!(tokens[4].start_ms, 2600);
        // 文末の語だけ sentence_end が立つ
        assert!(tokens[4].sentence_end);
        assert!(!tokens[0].sentence_end);
        // 採番は 0 から連番
        assert_eq!(tokens[4].index, 4);
    }

    #[test]
    fn empty_text_yields_no_tokens() {
        assert!(words_from_text("   ", 0, 1000).is_empty());
    }
}

#[cfg(test)]
mod newest_first_tests {
    use super::*;

    #[test]
    fn pops_newest_first_and_ignores_duplicates() {
        let mut queue = NewestFirst::default();
        queue.push(1);
        queue.push(2);
        queue.push(2);
        queue.push(3);
        assert_eq!(queue.len(), 3);
        assert_eq!(queue.pop_newest(), Some(3));
        queue.push(4);
        assert_eq!(queue.pop_newest(), Some(4));
        assert_eq!(queue.pop_newest(), Some(2));
        assert_eq!(queue.pop_newest(), Some(1));
        assert_eq!(queue.pop_newest(), None);
    }
}
