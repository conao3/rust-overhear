//! 辞書のストラテジー。
//!
//! 翻訳・ASR と同じく 1 本のトレイトの背後に隠す。英英 (WordNet) の他に
//! 英和や Yomitan 形式を足すときも、実装とレジストリ登録だけで済ませる。

use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub mod ejdict;
pub mod wordnet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Pos {
    Noun,
    Verb,
    Adjective,
    Adverb,
}

impl Pos {
    pub fn label(&self) -> &'static str {
        match self {
            Pos::Noun => "名詞",
            Pos::Verb => "動詞",
            Pos::Adjective => "形容詞",
            Pos::Adverb => "副詞",
        }
    }

    pub fn all() -> [Pos; 4] {
        [Pos::Noun, Pos::Verb, Pos::Adjective, Pos::Adverb]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictSense {
    pub definition: String,
    /// 同じ synset に属する語。
    pub synonyms: Vec<String>,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictEntry {
    /// 見出し語 (活用を解いた形)。
    pub lemma: String,
    /// 品詞。辞書によっては持たない (英和など)。
    pub pos: Option<Pos>,
    pub senses: Vec<DictSense>,
    pub source: String,
}

pub trait Dictionary: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &str;
    /// 表層形を受け取り、活用を解いて引く。見つからなければ空。
    fn lookup(&self, surface: &str) -> Vec<DictEntry>;
}

pub struct DictionaryRegistry {
    dictionaries: Vec<Arc<dyn Dictionary>>,
}

impl DictionaryRegistry {
    pub fn new(dictionaries: Vec<Arc<dyn Dictionary>>) -> Self {
        Self { dictionaries }
    }

    /// 環境から引けるものだけを登録する。辞書が無くても字幕は動く。
    pub fn from_env() -> Self {
        let mut dictionaries: Vec<Arc<dyn Dictionary>> = Vec::new();
        // 英和を先に置く。意味を掴むのが先で、英英は掘り下げ用。
        match ejdict::EjDict::from_env() {
            Ok(dict) => dictionaries.push(Arc::new(dict)),
            Err(err) => tracing::warn!(%err, "英和辞書を読み込めなかった"),
        }
        match wordnet::WordNet::from_env() {
            Ok(wn) => dictionaries.push(Arc::new(wn)),
            Err(err) => tracing::warn!(%err, "WordNet を読み込めなかった"),
        }
        Self { dictionaries }
    }

    pub fn is_empty(&self) -> bool {
        self.dictionaries.is_empty()
    }

    pub fn list(&self) -> &[Arc<dyn Dictionary>] {
        &self.dictionaries
    }

    /// 登録順に引き、見つかったものを全部返す。
    pub fn lookup(&self, surface: &str) -> Vec<DictEntry> {
        self.dictionaries
            .iter()
            .flat_map(|d| d.lookup(surface))
            .collect()
    }
}

impl Default for DictionaryRegistry {
    fn default() -> Self {
        Self::from_env()
    }
}

/// 字幕のトークンを辞書の見出し語に寄せる。
///
/// 句読点と所有格を落として小文字にする。WordNet のように自前の
/// 形態素解析を持つ辞書はこれを起点に、持たない辞書は
/// `inflection_candidates` と併用する。
pub fn normalize_surface(surface: &str) -> String {
    let trimmed = surface
        .trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '\'')
        .to_lowercase();
    trimmed
        .strip_suffix("'s")
        .map(str::to_string)
        .unwrap_or(trimmed)
}

/// 品詞を持たない辞書向けの、素朴な原形候補。
///
/// WordNet の例外リストのような裏付けが無いので、候補を出して
/// 辞書側に存在するものだけを採らせる。
pub fn inflection_candidates(word: &str) -> Vec<String> {
    const RULES: &[(&str, &str)] = &[
        ("ies", "y"),
        ("ches", "ch"),
        ("shes", "sh"),
        ("xes", "x"),
        ("sses", "ss"),
        ("es", ""),
        ("s", ""),
        ("ied", "y"),
        ("ed", ""),
        ("ed", "e"),
        ("ing", ""),
        ("ing", "e"),
        ("est", ""),
        ("er", ""),
    ];

    let mut out = vec![word.to_string()];
    for (suffix, replacement) in RULES {
        if let Some(stem) = word.strip_suffix(suffix) {
            let candidate = format!("{stem}{replacement}");
            if candidate.len() >= 2 && !out.contains(&candidate) {
                out.push(candidate);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_subtitle_tokens() {
        // 字幕のトークンは句読点がくっついてくる
        assert_eq!(normalize_surface("WELL,"), "well");
        assert_eq!(normalize_surface("dog's"), "dog");
        assert_eq!(normalize_surface("\"Stop!\""), "stop");
        assert_eq!(normalize_surface("well-known"), "well-known");
    }

    #[test]
    fn generates_plausible_base_forms() {
        let c = inflection_candidates("running");
        assert!(c.contains(&"running".to_string()));
        assert!(c.contains(&"runn".to_string()));
        assert!(c.contains(&"runne".to_string()));

        let c = inflection_candidates("studies");
        assert!(c.contains(&"study".to_string()));

        // 短すぎる候補は出さない
        assert!(!inflection_candidates("as").contains(&"a".to_string()));
    }
}
