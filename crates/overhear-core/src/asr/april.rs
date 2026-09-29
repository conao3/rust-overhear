//! april-asr (`april_api.h`) への FFI。
//!
//! Rust バインディングが存在しないため、公開 C API を直接宣言する。
//! 共有ライブラリ `libaprilasr.so` は nixpkgs の livecaptions の出力に
//! 同梱されており、flake の devShell が `APRIL_LIB_DIR` で指す。
//! モデルは `APRIL_MODEL_PATH`。

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;
use std::sync::Once;

use anyhow::{Context, Result, anyhow};
use tokio::sync::mpsc::UnboundedSender;

use super::{AsrEvent, AsrToken, Recognizer, tokens_to_text};

const APRIL_VERSION: c_int = 1;

const RESULT_RECOGNITION_PARTIAL: c_int = 1;
const RESULT_RECOGNITION_FINAL: c_int = 2;
const RESULT_ERROR_CANT_KEEP_UP: c_int = 3;
const RESULT_SILENCE: c_int = 4;

const TOKEN_FLAG_WORD_BOUNDARY: c_int = 0x0000_0001;
const TOKEN_FLAG_SENTENCE_END: c_int = 0x0000_0002;

/// 実時間で供給し、処理はバックグラウンドスレッドへ委ねる。
const CONFIG_FLAG_ASYNC_RT: c_int = 0x0000_0001;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct AprilSpeakerID {
    data: [u8; 16],
}

#[repr(C)]
struct AprilToken {
    token: *const c_char,
    logprob: f32,
    flags: c_int,
    time_ms: usize,
    reserved: *mut c_void,
}

type AprilResultHandler = extern "C" fn(*mut c_void, c_int, usize, *const AprilToken);

#[repr(C)]
struct AprilConfig {
    speaker: AprilSpeakerID,
    handler: Option<AprilResultHandler>,
    userdata: *mut c_void,
    flags: c_int,
}

extern "C" {
    fn aam_api_init(version: c_int);
    fn aam_create_model(model_path: *const c_char) -> *mut c_void;
    fn aam_get_sample_rate(model: *mut c_void) -> usize;
    fn aam_free(model: *mut c_void);
    fn aas_create_session(model: *mut c_void, config: AprilConfig) -> *mut c_void;
    fn aas_feed_pcm16(session: *mut c_void, pcm16: *mut i16, short_count: usize);
    fn aas_flush(session: *mut c_void);
    fn aas_free(session: *mut c_void);
}

static API_INIT: Once = Once::new();

struct Userdata {
    tx: UnboundedSender<AsrEvent>,
}

/// april-asr のバックグラウンドスレッドから呼ばれる。
extern "C" fn on_result(
    userdata: *mut c_void,
    result: c_int,
    count: usize,
    tokens: *const AprilToken,
) {
    if userdata.is_null() {
        return;
    }
    // Userdata は AprilRecognizer が session より長く生存させる。
    let ud = unsafe { &*(userdata as *const Userdata) };

    let event = match result {
        RESULT_SILENCE => AsrEvent::Silence,
        RESULT_ERROR_CANT_KEEP_UP => AsrEvent::CantKeepUp,
        RESULT_RECOGNITION_PARTIAL | RESULT_RECOGNITION_FINAL => {
            let parsed = unsafe { collect_tokens(count, tokens) };
            let text = tokens_to_text(&parsed);
            if result == RESULT_RECOGNITION_PARTIAL {
                AsrEvent::Partial {
                    text,
                    tokens: parsed,
                }
            } else {
                AsrEvent::Final {
                    text,
                    tokens: parsed,
                }
            }
        }
        _ => return,
    };

    // 受け手が落ちていても ASR 側は止めない。
    let _ = ud.tx.send(event);
}

/// # Safety
/// `tokens` は `count` 要素の有効な配列か、count が 0 のとき null。
/// 各 token の文字列ポインタは呼び出し中のみ有効なのでここでコピーする。
unsafe fn collect_tokens(count: usize, tokens: *const AprilToken) -> Vec<AsrToken> {
    if count == 0 || tokens.is_null() {
        return Vec::new();
    }
    let slice = std::slice::from_raw_parts(tokens, count);
    slice
        .iter()
        .map(|t| {
            let raw = if t.token.is_null() {
                String::new()
            } else {
                CStr::from_ptr(t.token).to_string_lossy().into_owned()
            };
            let surface = raw.trim().to_string();
            AsrToken {
                raw,
                surface,
                logprob: t.logprob,
                word_boundary: t.flags & TOKEN_FLAG_WORD_BOUNDARY != 0,
                sentence_end: t.flags & TOKEN_FLAG_SENTENCE_END != 0,
                time_ms: t.time_ms as u64,
            }
        })
        .collect()
}

pub struct AprilRecognizer {
    model: *mut c_void,
    session: *mut c_void,
    /// session より長生きさせる必要がある。Drop で解放する。
    userdata: *mut Userdata,
    sample_rate: u32,
}

// 生ポインタを含むが、所有権は 1 スレッドに閉じて扱う。
unsafe impl Send for AprilRecognizer {}

impl AprilRecognizer {
    /// `APRIL_MODEL_PATH` のモデルを読み込む。
    pub fn from_env(tx: UnboundedSender<AsrEvent>) -> Result<Self> {
        let path = std::env::var("APRIL_MODEL_PATH")
            .context("APRIL_MODEL_PATH が未設定 (nix develop の外で実行していないか)")?;
        Self::new(path, tx)
    }

    pub fn new(model_path: impl AsRef<Path>, tx: UnboundedSender<AsrEvent>) -> Result<Self> {
        let path = model_path.as_ref();
        if !path.exists() {
            return Err(anyhow!("april のモデルが見つからない: {}", path.display()));
        }
        API_INIT.call_once(|| unsafe { aam_api_init(APRIL_VERSION) });

        let c_path = CString::new(path.to_string_lossy().as_bytes())
            .context("モデルパスに NUL が含まれている")?;
        let model = unsafe { aam_create_model(c_path.as_ptr()) };
        if model.is_null() {
            return Err(anyhow!(
                "april のモデル読み込みに失敗した: {}",
                path.display()
            ));
        }

        let sample_rate = unsafe { aam_get_sample_rate(model) } as u32;
        let userdata = Box::into_raw(Box::new(Userdata { tx }));
        let config = AprilConfig {
            speaker: AprilSpeakerID::default(),
            handler: Some(on_result),
            userdata: userdata as *mut c_void,
            flags: CONFIG_FLAG_ASYNC_RT,
        };
        let session = unsafe { aas_create_session(model, config) };
        if session.is_null() {
            unsafe {
                drop(Box::from_raw(userdata));
                aam_free(model);
            }
            return Err(anyhow!("april のセッション生成に失敗した"));
        }

        tracing::info!(sample_rate, model = %path.display(), "april-asr を初期化した");
        Ok(Self {
            model,
            session,
            userdata,
            sample_rate,
        })
    }
}

impl Recognizer for AprilRecognizer {
    fn id(&self) -> &'static str {
        "april"
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn feed(&mut self, pcm: &[i16]) -> Result<()> {
        if pcm.is_empty() {
            return Ok(());
        }
        unsafe {
            aas_feed_pcm16(self.session, pcm.as_ptr() as *mut i16, pcm.len());
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        unsafe { aas_flush(self.session) };
        Ok(())
    }
}

impl Drop for AprilRecognizer {
    fn drop(&mut self) {
        unsafe {
            // session -> model -> userdata の順で解放する。
            aas_free(self.session);
            aam_free(self.model);
            drop(Box::from_raw(self.userdata));
        }
    }
}
