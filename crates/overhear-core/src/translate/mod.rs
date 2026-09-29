//! 翻訳エンジンのストラテジー。
//!
//! 既定はローカルモデル (Ollama)。DeepL / Google Translate は必要に応じて
//! ユーザーが選ぶ。呼び出し側はどの実装が動いているかを知らない。

use std::sync::{Arc, RwLock};

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
    /// 初回の翻訳が遅れないよう、モデルの読み込み等を先に済ませる。
    async fn warm_up(&self) -> Result<(), TranslateError> {
        Ok(())
    }
}

/// 言語コードを英語の言語名にする。
///
/// LLM に渡すプロンプトは `ja` のようなコードだと小さいモデルが解釈を
/// 誤り、別の言語で返してくる。名前で渡す。
pub fn language_name(code: &str) -> &str {
    match code.to_ascii_lowercase().split(['-', '_']).next() {
        Some("ja") => "Japanese",
        Some("en") => "English",
        Some("zh") => "Chinese",
        Some("ko") => "Korean",
        Some("es") => "Spanish",
        Some("fr") => "French",
        Some("de") => "German",
        Some("it") => "Italian",
        Some("pt") => "Portuguese",
        Some("ru") => "Russian",
        _ => code,
    }
}

/// 登録済みストラテジーの一覧と、既定エンジンの解決を持つ。
pub struct TranslatorRegistry {
    engines: Vec<Arc<dyn Translator>>,
    /// モデルの切り替えのため、型のまま持っておく。
    ollama: Arc<ollama::OllamaTranslator>,
    /// 実行中に UI から切り替えられる。
    default_id: RwLock<String>,
}

impl TranslatorRegistry {
    /// 既定の顔ぶれ。ローカルの Ollama を既定エンジンに据える。
    pub fn with_defaults(keys: Arc<crate::secrets::ApiKeys>) -> anyhow::Result<Self> {
        let ollama = Arc::new(ollama::OllamaTranslator::from_env()?);
        let engines: Vec<Arc<dyn Translator>> = vec![
            ollama.clone(),
            Arc::new(deepl::DeeplTranslator::new(Arc::clone(&keys))),
            Arc::new(google::GoogleTranslator::new(keys)),
            Arc::new(null::NullTranslator),
        ];
        Ok(Self {
            engines,
            ollama,
            default_id: RwLock::new("ollama".to_string()),
        })
    }

    /// 既定エンジンを切り替える。登録されていない id は拒む。
    pub fn set_default(&self, id: &str) -> anyhow::Result<()> {
        if self.get(id).is_none() {
            anyhow::bail!("翻訳エンジン {id} は登録されていない");
        }
        let mut default_id = self
            .default_id
            .write()
            .map_err(|_| anyhow::anyhow!("lock poisoned"))?;
        *default_id = id.to_string();
        Ok(())
    }

    pub fn default_id(&self) -> String {
        self.default_id
            .read()
            .map(|id| id.clone())
            .unwrap_or_default()
    }

    pub fn list(&self) -> &[Arc<dyn Translator>] {
        &self.engines
    }

    /// 既定エンジンの準備を済ませる。
    pub async fn warm_up_default(&self) {
        let Some(engine) = self.get(&self.default_id()) else {
            return;
        };
        let started = std::time::Instant::now();
        match engine.warm_up().await {
            Ok(()) => tracing::info!(
                engine = engine.id(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "翻訳エンジンの準備が済んだ"
            ),
            Err(err) => {
                tracing::warn!(engine = engine.id(), %err, "翻訳エンジンを準備できなかった")
            }
        }
    }

    pub fn ollama(&self) -> &ollama::OllamaTranslator {
        &self.ollama
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
        let default_id = self.default_id();
        let wanted = engine_id.unwrap_or(&default_id);
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

        if wanted == default_id {
            return None;
        }
        let fallback = self.get(&default_id)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_name_resolves_codes_and_regions() {
        assert_eq!(language_name("ja"), "Japanese");
        assert_eq!(language_name("en-US"), "English");
        assert_eq!(language_name("zh_TW"), "Chinese");
        assert_eq!(language_name("tlh"), "tlh");
    }

    #[test]
    fn default_engine_switches_only_to_registered_ids() {
        let keys = crate::secrets::ApiKeys::load(
            Box::new(crate::secrets::tests::MemoryBackend::default()),
            &[],
        )
        .unwrap();
        let registry = TranslatorRegistry::with_defaults(Arc::new(keys)).unwrap();
        assert_eq!(registry.default_id(), "ollama");
        registry.set_default("none").unwrap();
        assert_eq!(registry.default_id(), "none");
        assert!(registry.set_default("nope").is_err());
        assert_eq!(registry.default_id(), "none");
    }
}
