//! DeepL API によるストラテジー。品質を優先して名指しで選ぶとき用。
//!
//! API キーは Secret Service に置く ([`crate::secrets`])。

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use crate::secrets::ApiKeys;

use super::{Availability, TranslateError, TranslateRequest, Translator, TranslatorCapabilities};

pub struct DeeplTranslator {
    keys: Arc<ApiKeys>,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct DeeplResponse {
    translations: Vec<DeeplTranslation>,
}

#[derive(Deserialize)]
struct DeeplTranslation {
    text: String,
}

impl DeeplTranslator {
    pub fn new(keys: Arc<ApiKeys>) -> Self {
        Self {
            keys,
            client: reqwest::Client::new(),
        }
    }

    /// Free プランのキーは `:fx` で終わる。
    fn endpoint(key: &str) -> &'static str {
        if key.ends_with(":fx") {
            "https://api-free.deepl.com/v2/translate"
        } else {
            "https://api.deepl.com/v2/translate"
        }
    }
}

#[async_trait]
impl Translator for DeeplTranslator {
    fn id(&self) -> &'static str {
        "deepl"
    }

    fn display_name(&self) -> &str {
        "DeepL"
    }

    fn capabilities(&self) -> TranslatorCapabilities {
        TranslatorCapabilities {
            sends_data_externally: true,
            supported_target_langs: vec!["ja".into(), "en-us".into(), "de".into(), "fr".into()],
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

        let mut form = vec![
            ("text", req.text.clone()),
            ("target_lang", req.target_lang.to_uppercase()),
        ];
        if let Some(src) = &req.source_lang {
            form.push(("source_lang", src.to_uppercase()));
        }

        let resp = self
            .client
            .post(Self::endpoint(&key))
            .header("Authorization", format!("DeepL-Auth-Key {key}"))
            .form(&form)
            .send()
            .await
            .map_err(|e| TranslateError::Unavailable(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(TranslateError::Failed(format!("HTTP {}", resp.status())));
        }
        let parsed: DeeplResponse = resp
            .json()
            .await
            .map_err(|e| TranslateError::Failed(e.to_string()))?;
        parsed
            .translations
            .into_iter()
            .next()
            .map(|t| t.text)
            .ok_or_else(|| TranslateError::Failed("訳文が空だった".into()))
    }
}
