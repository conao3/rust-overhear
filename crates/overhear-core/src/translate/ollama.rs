//! ローカルの Ollama による翻訳。**既定のストラテジー**。
//!
//! ビデオ会議の音声まで拾う設計であるため、既定で外部へ送らない。

use async_trait::async_trait;
use serde::Deserialize;

use super::{Availability, TranslateError, TranslateRequest, Translator, TranslatorCapabilities};

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:11434";
const DEFAULT_MODEL: &str = "qwen3:8b";

pub struct OllamaTranslator {
    endpoint: String,
    model: String,
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
    pub fn from_env() -> Self {
        Self {
            endpoint: std::env::var("OVERHEAR_OLLAMA_ENDPOINT")
                .unwrap_or_else(|_| DEFAULT_ENDPOINT.to_string()),
            model: std::env::var("OVERHEAR_OLLAMA_MODEL")
                .unwrap_or_else(|_| DEFAULT_MODEL.to_string()),
            client: reqwest::Client::new(),
        }
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
        let url = format!("{}/api/tags", self.endpoint);
        match self.client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => match resp.json::<TagsResponse>().await {
                Ok(tags) => {
                    if tags.models.iter().any(|m| m.name.starts_with(&self.model)) {
                        Availability::ok()
                    } else {
                        Availability::unavailable(format!(
                            "モデル {} が pull されていない",
                            self.model
                        ))
                    }
                }
                Err(err) => Availability::unavailable(format!("応答を解釈できない: {err}")),
            },
            Ok(resp) => Availability::unavailable(format!("Ollama が {} を返した", resp.status())),
            Err(_) => Availability::unavailable(format!("{} に接続できない", self.endpoint)),
        }
    }

    async fn translate(&self, req: &TranslateRequest) -> Result<String, TranslateError> {
        let prompt = format!(
            "Translate the following text into {}. Output only the translation, \
             with no explanation, no quotes, and no preamble.\n\n{}",
            req.target_lang, req.text
        );
        let body = serde_json::json!({
            "model": self.model,
            "prompt": prompt,
            "stream": false,
            "think": false,
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
