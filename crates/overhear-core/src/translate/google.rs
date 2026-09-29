//! Google Cloud Translation API によるストラテジー。
//! DeepL が対応しない言語ペア向け。

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use crate::secrets::ApiKeys;

use super::{Availability, TranslateError, TranslateRequest, Translator, TranslatorCapabilities};

const ENDPOINT: &str = "https://translation.googleapis.com/language/translate/v2";

pub struct GoogleTranslator {
    keys: Arc<ApiKeys>,
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
    pub fn new(keys: Arc<ApiKeys>) -> Self {
        Self {
            keys,
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
        match self.keys.get(self.id()) {
            Some(_) => Availability::ok(),
            None => Availability::unavailable("API キーが未設定 (設定タブで入れる)"),
        }
    }

    async fn translate(&self, req: &TranslateRequest) -> Result<String, TranslateError> {
        let key = self
            .keys
            .get(self.id())
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
