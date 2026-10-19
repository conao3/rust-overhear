//! 辞書のストラテジー。
//!
//! 翻訳・ASR と同じく 1 本のトレイトの背後に隠す。英英 (WordNet) の他に
//! 英和や Yomitan 形式を足すときも、実装とレジストリ登録だけで済ませる。

use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub mod ejdict;
pub mod ipa;
pub mod ruby;
pub mod wordnet;
pub mod wordnet_ja;

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
    #[serde(default)]
    pub ipa_us: Option<String>,
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
    pronunciation: ipa::IpaDict,
    pub ruby: ruby::RubyCache,
}

impl DictionaryRegistry {
    pub fn new(dictionaries: Vec<Arc<dyn Dictionary>>) -> Self {
        Self {
            dictionaries,
            pronunciation: ipa::IpaDict::default(),
            ruby: ruby::RubyCache::default(),
        }
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
            Ok(wn) => {
                let wn = Arc::new(wn);
                match wordnet_ja::JapaneseWordNet::from_env(wn.clone()) {
                    Ok(dict) => dictionaries.push(Arc::new(dict)),
                    Err(err) => tracing::warn!(%err, "日本語 WordNet を読み込めなかった"),
                }
                dictionaries.push(wn);
            }
            Err(err) => tracing::warn!(%err, "WordNet を読み込めなかった"),
        }
        let pronunciation = ipa::IpaDict::from_env().unwrap_or_else(|err| {
            tracing::warn!(%err, "発音辞書を読み込めなかった");
            ipa::IpaDict::default()
        });
        Self {
            dictionaries,
            pronunciation,
            ruby: ruby::RubyCache::default(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.dictionaries.is_empty()
    }

    pub fn list(&self) -> &[Arc<dyn Dictionary>] {
        &self.dictionaries
    }

    /// 登録順に引き、見つかったものを全部返す。
    pub fn lookup(&self, surface: &str) -> Vec<DictEntry> {
        let direct: Vec<Vec<DictEntry>> = self
            .dictionaries
            .iter()
            .map(|d| d.lookup(surface))
            .collect();
        let mut lemmas = Vec::new();
        for entry in direct.iter().flatten() {
            if !lemmas.contains(&entry.lemma) {
                lemmas.push(entry.lemma.clone());
            }
        }
        self.dictionaries
            .iter()
            .zip(direct)
            .flat_map(|(dictionary, mut entries)| {
                for lemma in &lemmas {
                    if entries.iter().any(|entry| entry.lemma == *lemma) {
                        continue;
                    }
                    for entry in dictionary.lookup(lemma) {
                        if !entries.iter().any(|existing| {
                            existing.lemma == entry.lemma && existing.pos == entry.pos
                        }) {
                            entries.push(entry);
                        }
                    }
                }
                entries
            })
            .map(|mut entry| {
                entry.ipa_us = self.pronunciation.lookup(&entry.lemma).map(str::to_owned);
                entry
            })
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
        .replace(['’', '‘'], "'")
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
        assert_eq!(normalize_surface("“Dog’s,”"), "dog");
        assert_eq!(normalize_surface("don’t"), "don't");
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

    struct TestDictionary(&'static str);

    impl Dictionary for TestDictionary {
        fn id(&self) -> &'static str {
            self.0
        }
        fn display_name(&self) -> &str {
            self.0
        }
        fn lookup(&self, surface: &str) -> Vec<DictEntry> {
            let lemma = match (self.0, surface) {
                ("english", "went") => "go",
                (_, "go") => "go",
                (_, "running") => "running",
                (_, "run") => "run",
                _ => return Vec::new(),
            };
            let mut entries = vec![DictEntry {
                lemma: lemma.into(),
                pos: None,
                ipa_us: None,
                senses: vec![],
                source: self.0.into(),
            }];
            if self.0 == "english" && surface == "running" {
                entries.extend(self.lookup("run"));
            }
            entries
        }
    }

    #[test]
    fn shares_lemmas_between_dictionaries_and_preserves_order() {
        let dictionaries = DictionaryRegistry::new(vec![
            Arc::new(TestDictionary("japanese")),
            Arc::new(TestDictionary("english")),
        ]);
        let entries = dictionaries.lookup("went");
        assert_eq!(
            entries
                .iter()
                .map(|e| (e.source.as_str(), e.lemma.as_str()))
                .collect::<Vec<_>>(),
            vec![("japanese", "go"), ("english", "go")]
        );
        let entries = dictionaries.lookup("running");
        assert_eq!(
            entries
                .iter()
                .map(|e| (e.source.as_str(), e.lemma.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("japanese", "running"),
                ("japanese", "run"),
                ("english", "running"),
                ("english", "run")
            ]
        );
    }
    #[test]
    fn pronunciation_matches_the_headword_after_inflection_resolution() {
        let mut dictionaries = DictionaryRegistry::new(vec![
            Arc::new(TestDictionary("japanese")),
            Arc::new(TestDictionary("english")),
        ]);
        dictionaries.pronunciation = ipa::IpaDict::read(&b"went\t/went/\ngo\t/go/\n"[..]).unwrap();
        let entries = dictionaries.lookup("went");
        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .all(|e| e.lemma == "go" && e.ipa_us.as_deref() == Some("/go/"))
        );
        assert!(
            dictionaries
                .lookup("running")
                .iter()
                .all(|e| e.ipa_us.is_none())
        );
    }
}
