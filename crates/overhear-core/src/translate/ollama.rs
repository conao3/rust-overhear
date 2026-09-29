//! ローカルの Ollama による翻訳。**既定のストラテジー**。
//!
//! ビデオ会議の音声まで拾う設計であるため、既定で外部へ送らない。

use anyhow::Context as _;
use async_trait::async_trait;
use serde::Deserialize;

use super::{
    Availability, TranslateError, TranslateRequest, Translator, TranslatorCapabilities,
    language_name,
};

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:11434";
const DEFAULT_MODEL: &str = "qwen3:8b";
/// 常駐アプリなので、発話の合間にモデルが降ろされないよう長めに持たせる。
const DEFAULT_KEEP_ALIVE: &str = "30m";

pub struct OllamaTranslator {
    endpoint: String,
    /// 設定から実行中に切り替えられる。
    model: std::sync::RwLock<String>,
    keep_alive: String,
    /// 推論スレッド数。None なら Ollama の判断に任せる。
    num_thread: Option<u32>,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct GenerateResponse {
    response: String,
}

#[derive(Deserialize)]
struct TagsResponse {
    models: Vec<TagModel>,
}

#[derive(Deserialize)]
struct TagModel {
    name: String,
}

impl OllamaTranslator {
    pub fn from_env() -> anyhow::Result<Self> {
        let num_thread =
            match std::env::var("OVERHEAR_OLLAMA_NUM_THREAD") {
                Ok(value) => Some(value.parse::<u32>().with_context(|| {
                    format!("OVERHEAR_OLLAMA_NUM_THREAD が数値でない: {value}")
                })?),
                Err(_) => None,
            };
        Ok(Self {
            endpoint: std::env::var("OVERHEAR_OLLAMA_ENDPOINT")
                .unwrap_or_else(|_| DEFAULT_ENDPOINT.to_string()),
            model: std::sync::RwLock::new(
                std::env::var("OVERHEAR_OLLAMA_MODEL")
                    .unwrap_or_else(|_| DEFAULT_MODEL.to_string()),
            ),
            keep_alive: std::env::var("OVERHEAR_OLLAMA_KEEP_ALIVE")
                .unwrap_or_else(|_| DEFAULT_KEEP_ALIVE.to_string()),
            num_thread,
            client: reqwest::Client::new(),
        })
    }

    /// 原文を先に、指示を後に置く。
    ///
    /// qwen3 のテンプレートは `think: false` のときユーザー発話の末尾に
    /// ` /no_think` を足す。原文で終わるプロンプトだとそれを原文の一部として
    /// 訳に混ぜてくる。
    pub fn model(&self) -> String {
        self.model.read().map(|m| m.clone()).unwrap_or_default()
    }

    pub fn set_model(&self, model: &str) -> anyhow::Result<()> {
        let mut current = self
            .model
            .write()
            .map_err(|_| anyhow::anyhow!("lock poisoned"))?;
        *current = model.to_string();
        Ok(())
    }

    /// pull 済みのモデル名。
    pub async fn installed_models(&self) -> anyhow::Result<Vec<String>> {
        let tags: TagsResponse = self
            .client
            .get(format!("{}/api/tags", self.endpoint))
            .send()
            .await
            .with_context(|| format!("{} に接続できない", self.endpoint))?
            .error_for_status()?
            .json()
            .await?;
        Ok(tags.models.into_iter().map(|m| m.name).collect())
    }

    fn prompt(text: &str, target_lang: &str) -> String {
        format!(
            "Text:\n{}\n\nTranslate the text above into {}. Output only the translation, \
             with no explanation, no quotes, and no preamble.",
            text,
            language_name(target_lang)
        )
    }

    async fn generate(&self, prompt: &str) -> Result<String, TranslateError> {
        let mut options = serde_json::json!({ "temperature": 0 });
        if let Some(n) = self.num_thread {
            options["num_thread"] = n.into();
        }
        let body = serde_json::json!({
            "model": self.model(),
            "prompt": prompt,
            "stream": false,
            "think": false,
            "keep_alive": self.keep_alive,
            "options": options,
        });
        let resp = self
            .client
            .post(format!("{}/api/generate", self.endpoint))
            .json(&body)
            .send()
            .await
            .map_err(|e| TranslateError::Unavailable(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(TranslateError::Failed(format!("HTTP {}", resp.status())));
        }
        let parsed: GenerateResponse = resp
            .json()
            .await
            .map_err(|e| TranslateError::Failed(e.to_string()))?;
        Ok(parsed.response.trim().to_string())
    }
}

#[async_trait]
impl Translator for OllamaTranslator {
    fn id(&self) -> &'static str {
        "ollama"
    }

    fn display_name(&self) -> &str {
        "Ollama (ローカル)"
    }

    fn capabilities(&self) -> TranslatorCapabilities {
        TranslatorCapabilities {
            sends_data_externally: false,
            supported_target_langs: vec!["ja".into(), "en".into()],
            max_chars_per_request: 4000,
        }
    }

    async fn availability(&self) -> Availability {
        let model = self.model();
        match self.installed_models().await {
            Ok(models) if models.iter().any(|m| m.starts_with(&model)) => Availability::ok(),
            Ok(_) => Availability::unavailable(format!("モデル {model} が pull されていない")),
            Err(err) => Availability::unavailable(err.to_string()),
        }
    }

    async fn translate(&self, req: &TranslateRequest) -> Result<String, TranslateError> {
        self.generate(&Self::prompt(&req.text, &req.target_lang))
            .await
    }

    /// モデルをメモリへ載せる。CPU 推論では初回だけ読み込みで数秒余計にかかる。
    async fn warm_up(&self) -> Result<(), TranslateError> {
        self.generate(&Self::prompt("Hello.", "ja"))
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_names_the_language() {
        let prompt = OllamaTranslator::prompt("Hi.", "ja");
        assert!(prompt.starts_with("Text:\nHi.\n\n"));
        assert!(prompt.contains("into Japanese."));
    }
}
