//! 翻訳しないストラテジー。
//!
//! 「原文だけ見たい」を分岐ではなく実装の 1 つとして吸収する。

use async_trait::async_trait;

use super::{Availability, TranslateError, TranslateRequest, Translator, TranslatorCapabilities};

pub struct NullTranslator;

#[async_trait]
impl Translator for NullTranslator {
    fn id(&self) -> &'static str {
        "none"
    }

    fn display_name(&self) -> &str {
        "翻訳しない"
    }

    fn capabilities(&self) -> TranslatorCapabilities {
        TranslatorCapabilities {
            sends_data_externally: false,
            supported_target_langs: vec![],
            max_chars_per_request: usize::MAX,
        }
    }

    async fn availability(&self) -> Availability {
        Availability::ok()
    }

    async fn translate(&self, _req: &TranslateRequest) -> Result<String, TranslateError> {
        Ok(String::new())
    }
}
