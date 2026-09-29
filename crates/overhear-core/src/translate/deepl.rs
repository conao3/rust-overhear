//! DeepL API によるストラテジー。品質を優先して名指しで選ぶとき用。
//!
//! API キーは MVP では環境変数から読む。Secret Service (keyring) への
//! 移行はフェーズ 4。

use async_trait::async_trait;
use serde::Deserialize;

use super::{Availability, TranslateError, TranslateRequest, Translator, TranslatorCapabilities};

pub struct DeeplTranslator {
    api_key: Option<String>,
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
    pub fn from_env() -> Self {
        Self {
            api_key: std::env::var("OVERHEAR_DEEPL_API_KEY").ok(),
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
        match &self.api_key {
            Some(_) => Availability::ok(),
            None => Availability::unavailable("OVERHEAR_DEEPL_API_KEY が未設定"),
        }
    }

    async fn translate(&self, req: &TranslateRequest) -> Result<String, TranslateError> {
        let key = self
            .api_key
            .as_ref()
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
            .post(Self::endpoint(key))
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
