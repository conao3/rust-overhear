//! Google Cloud Translation API によるストラテジー。
//! DeepL が対応しない言語ペア向け。

use async_trait::async_trait;
use serde::Deserialize;

use super::{Availability, TranslateError, TranslateRequest, Translator, TranslatorCapabilities};

const ENDPOINT: &str = "https://translation.googleapis.com/language/translate/v2";

pub struct GoogleTranslator {
    api_key: Option<String>,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct GoogleResponse {
    data: GoogleData,
}

#[derive(Deserialize)]
struct GoogleData {
    translations: Vec<GoogleTranslation>,
}

#[derive(Deserialize)]
struct GoogleTranslation {
    #[serde(rename = "translatedText")]
    translated_text: String,
}

impl GoogleTranslator {
    pub fn from_env() -> Self {
        Self {
            api_key: std::env::var("OVERHEAR_GOOGLE_API_KEY").ok(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Translator for GoogleTranslator {
    fn id(&self) -> &'static str {
        "google"
    }

    fn display_name(&self) -> &str {
        "Google Translate"
    }

    fn capabilities(&self) -> TranslatorCapabilities {
        TranslatorCapabilities {
            sends_data_externally: true,
            supported_target_langs: vec!["ja".into(), "en".into(), "ko".into(), "zh".into()],
            max_chars_per_request: 5000,
        }
    }

    async fn availability(&self) -> Availability {
        match &self.api_key {
            Some(_) => Availability::ok(),
            None => Availability::unavailable("OVERHEAR_GOOGLE_API_KEY が未設定"),
        }
    }

    async fn translate(&self, req: &TranslateRequest) -> Result<String, TranslateError> {
        let key = self
            .api_key
            .as_ref()
            .ok_or_else(|| TranslateError::Unavailable("API キーが未設定".into()))?;

        let mut body = serde_json::json!({
            "q": req.text,
            "target": req.target_lang,
            "format": "text",
        });
        if let Some(src) = &req.source_lang {
            body["source"] = serde_json::Value::String(src.clone());
        }

        let resp = self
            .client
            .post(ENDPOINT)
            .query(&[("key", key)])
            .json(&body)
            .send()
            .await
            .map_err(|e| TranslateError::Unavailable(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(TranslateError::Failed(format!("HTTP {}", resp.status())));
        }
        let parsed: GoogleResponse = resp
            .json()
            .await
            .map_err(|e| TranslateError::Failed(e.to_string()))?;
        parsed
            .data
            .translations
            .into_iter()
            .next()
            .map(|t| t.translated_text)
            .ok_or_else(|| TranslateError::Failed("訳文が空だった".into()))
    }
}
