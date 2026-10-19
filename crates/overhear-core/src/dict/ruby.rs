use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use super::{DictionaryRegistry, normalize_surface};

static FREQUENCIES: LazyLock<HashMap<&'static str, i32>> = LazyLock::new(|| {
    include_str!("../../data/en-frequency.txt")
        .lines()
        .enumerate()
        .map(|(index, word)| (word, index as i32 + 1))
        .collect()
});

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WordRuby {
    pub text: String,
    pub rank: i32,
}

#[derive(Default)]
pub struct RubyCache(Mutex<HashMap<String, Option<WordRuby>>>);

impl RubyCache {
    pub fn lookup(&self, dictionaries: &DictionaryRegistry, surface: &str) -> Option<WordRuby> {
        let word = normalize_surface(surface);
        if word.is_empty() || word.len() > 128 {
            return None;
        }
        let mut cache = self.0.lock().expect("ruby cache lock poisoned");
        if let Some(ruby) = cache.get(&word) {
            return ruby.clone();
        }
        let entries = dictionaries.lookup(&word);
        let rank = entries
            .iter()
            .map(|e| e.lemma.as_str())
            .chain([word.as_str()])
            .filter_map(|lemma| FREQUENCIES.get(lemma).copied())
            .min()
            .unwrap_or(10001);
        let ruby = entries
            .iter()
            .filter(|e| matches!(e.source.as_str(), "ejdict" | "wordnet-ja"))
            .flat_map(|e| &e.senses)
            .map(|sense| compact_gloss(&sense.definition))
            .find(|text| !text.is_empty())
            .map(|text| WordRuby { text, rank });
        if cache.len() >= 8192 {
            cache.clear();
        }
        cache.insert(word, ruby.clone());
        ruby
    }
}

fn compact_gloss(definition: &str) -> String {
    let mut text = String::new();
    let mut closing = Vec::new();
    for c in definition.chars() {
        if let Some(end) = match c {
            '《' => Some('》'),
            '〈' => Some('〉'),
            '(' => Some(')'),
            '（' => Some('）'),
            _ => None,
        } {
            closing.push(end);
        } else if closing.last() == Some(&c) {
            closing.pop();
        } else if closing.is_empty() && !matches!(c, '『' | '』' | '\'' | '"') {
            if matches!(c, ';' | '；' | '、' | '，' | ',' | '。' | '\n') {
                break;
            }
            text.push(c);
        }
    }
    let text = text.trim();
    if !text
        .chars()
        .any(|c| ('\u{3040}'..='\u{30ff}').contains(&c) || ('\u{3400}'..='\u{9fff}').contains(&c))
    {
        return String::new();
    }
    let mut short: String = text.chars().take(12).collect();
    if text.chars().count() > 12 {
        short.push('…');
    }
    short
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::{DictEntry, DictSense, Dictionary};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct CountingDictionary(Arc<AtomicUsize>);
    impl Dictionary for CountingDictionary {
        fn id(&self) -> &'static str {
            "ejdict"
        }
        fn display_name(&self) -> &str {
            "ejdict"
        }
        fn lookup(&self, word: &str) -> Vec<DictEntry> {
            self.0.fetch_add(1, Ordering::Relaxed);
            if word != "thesis" {
                return vec![];
            }
            vec![DictEntry {
                lemma: word.into(),
                pos: None,
                ipa_us: None,
                source: "ejdict".into(),
                senses: vec![DictSense {
                    definition: "『論文』,卒業論文".into(),
                    synonyms: vec![],
                    examples: vec![],
                }],
            }]
        }
    }

    #[test]
    fn caches_normalized_words_and_misses() {
        let calls = Arc::new(AtomicUsize::new(0));
        let dict = DictionaryRegistry::new(vec![Arc::new(CountingDictionary(calls.clone()))]);
        let ruby = dict.ruby.lookup(&dict, "THESIS,").unwrap();
        assert_eq!(ruby.text, "論文");
        assert!(ruby.rank > 1000);
        assert_eq!(dict.ruby.lookup(&dict, "thesis").unwrap(), ruby);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert!(dict.ruby.lookup(&dict, "unknownword").is_none());
        assert!(dict.ruby.lookup(&dict, "UNKNOWNWORD!").is_none());
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn short_gloss_preserves_japanese_and_omits_usage_labels() {
        assert_eq!(compact_gloss("《複数形》『論文』,学位論文"), "論文");
        assert_eq!(compact_gloss("(物を)聞く;聞こえる"), "聞く");
        assert_eq!(compact_gloss("= example"), "");
        assert_eq!(
            compact_gloss("あいうえおかきくけこさしすせそ"),
            "あいうえおかきくけこさし…"
        );
        assert!(FREQUENCIES["the"] < 1000);
    }
}
