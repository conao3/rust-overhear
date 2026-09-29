//! 翻訳エンジンのストラテジー。
//!
//! 既定はローカルモデル (Ollama)。DeepL / Google Translate は必要に応じて
//! ユーザーが選ぶ。呼び出し側はどの実装が動いているかを知らない。

use std::sync::Arc;

use async_trait::async_trait;

use crate::model::Translation;

pub mod deepl;
pub mod google;
pub mod null;
pub mod ollama;

#[derive(Debug, Clone)]
pub struct TranslatorCapabilities {
    /// 外部へ本文を送るか。UI でローカルと外部を分けて見せるために使う。
    pub sends_data_externally: bool,
    pub supported_target_langs: Vec<String>,
    pub max_chars_per_request: usize,
}

#[derive(Debug, Clone)]
pub struct Availability {
    pub available: bool,
    /// 使えない理由 (API キー未設定、接続先が無い等)。
    pub reason: Option<String>,
}

impl Availability {
    pub fn ok() -> Self {
        Self {
            available: true,
            reason: None,
        }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            available: false,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TranslateRequest {
    pub text: String,
    pub source_lang: Option<String>,
    pub target_lang: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TranslateError {
    #[error("翻訳エンジンが利用できない: {0}")]
    Unavailable(String),
    #[error("翻訳に失敗した: {0}")]
    Failed(String),
}

#[async_trait]
pub trait Translator: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &str;
    fn capabilities(&self) -> TranslatorCapabilities;
    /// 資格情報・接続先が揃っているか。UI の選択肢の活性/非活性に使う。
    async fn availability(&self) -> Availability;
    async fn translate(&self, req: &TranslateRequest) -> Result<String, TranslateError>;
}

/// 登録済みストラテジーの一覧と、既定エンジンの解決を持つ。
pub struct TranslatorRegistry {
    engines: Vec<Arc<dyn Translator>>,
    default_id: String,
}

impl TranslatorRegistry {
    /// 既定の顔ぶれ。ローカルの Ollama を既定エンジンに据える。
    pub fn with_defaults() -> Self {
        let engines: Vec<Arc<dyn Translator>> = vec![
            Arc::new(ollama::OllamaTranslator::from_env()),
            Arc::new(deepl::DeeplTranslator::from_env()),
            Arc::new(google::GoogleTranslator::from_env()),
            Arc::new(null::NullTranslator),
        ];
        Self {
            engines,
            default_id: "ollama".to_string(),
        }
    }

    pub fn set_default(&mut self, id: impl Into<String>) {
        self.default_id = id.into();
    }

    pub fn default_id(&self) -> &str {
        &self.default_id
    }

    pub fn list(&self) -> &[Arc<dyn Translator>] {
        &self.engines
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Translator>> {
        self.engines.iter().find(|e| e.id() == id).cloned()
    }

    /// 指定エンジンで翻訳し、失敗したら既定エンジンへフォールバックする。
    /// フォールバックしたことは Translation::fallback_from に残す。
    pub async fn translate(
        &self,
        engine_id: Option<&str>,
        req: &TranslateRequest,
    ) -> Option<Translation> {
        let wanted = engine_id.unwrap_or(&self.default_id);
        if let Some(engine) = self.get(wanted) {
            match engine.translate(req).await {
                Ok(text) => {
                    return Some(Translation {
                        engine_id: engine.id().to_string(),
                        text,
                        target_lang: req.target_lang.clone(),
                        fallback_from: None,
                    });
                }
                Err(err) => {
                    tracing::warn!(engine = wanted, %err, "翻訳に失敗したのでフォールバックする");
                }
            }
        }

        if wanted == self.default_id {
            return None;
        }
        let fallback = self.get(&self.default_id)?;
        match fallback.translate(req).await {
            Ok(text) => Some(Translation {
                engine_id: fallback.id().to_string(),
                text,
                target_lang: req.target_lang.clone(),
                fallback_from: Some(wanted.to_string()),
            }),
            Err(err) => {
                // 翻訳は落とすが字幕そのものは止めない。
                tracing::warn!(%err, "フォールバック先の翻訳も失敗した");
                None
            }
        }
    }
}

impl Default for TranslatorRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}
