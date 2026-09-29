//! 辞書のストラテジー。
//!
//! 翻訳・ASR と同じく 1 本のトレイトの背後に隠す。英英 (WordNet) の他に
//! 英和や Yomitan 形式を足すときも、実装とレジストリ登録だけで済ませる。

use std::sync::Arc;

use serde::{Deserialize, Serialize};

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
    pub pos: Pos,
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
