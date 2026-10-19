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
        if is_contraction(surface) {
            return None;
        }
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

fn is_contraction(surface: &str) -> bool {
    let word = surface
        .replace(['’', '‘'], "'")
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    ["n't", "'m", "'re", "'ve", "'ll", "'d"]
        .iter()
        .any(|suffix| word.ends_with(suffix))
        || matches!(
            word.as_str(),
            "it's"
                | "he's"
                | "she's"
                | "that's"
                | "there's"
                | "here's"
                | "what's"
                | "who's"
                | "how's"
                | "where's"
                | "when's"
                | "why's"
                | "let's"
        )
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
            '[' => Some(']'),
            '［' => Some('］'),
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
    if ["の省略形", "の短縮形", "の縮約形", "の略形", "の略語"]
        .iter()
        .any(|description| text.contains(description))
        || text.ends_with("の略")
        || text.starts_with("短縮形")
    {
        return String::new();
    }
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

    #[test]
    fn omits_contraction_explanations_but_keeps_translations() {
        for definition in [
            "it isの省略形",
            "do notの短縮形",
            "going toの縮約形",
            "ABCの略",
            "短縮形(has, is, does)",
        ] {
            assert_eq!(compact_gloss(definition), "", "{definition}");
        }
        assert_eq!(compact_gloss("『写真』[photographの短縮形]"), "写真");
        assert_eq!(compact_gloss("チェロ(楽器)[violoncelloの短縮形]"), "チェロ");
        assert_eq!(compact_gloss("省略形"), "省略形");
        assert_eq!(compact_gloss("戦争の略奪品"), "戦争の略奪品");
    }

    #[test]
    fn contractions_are_filtered_before_possessive_normalization_and_cache_lookup() {
        let calls = Arc::new(AtomicUsize::new(0));
        let dict = DictionaryRegistry::new(vec![Arc::new(CountingDictionary(calls.clone()))]);
        dict.ruby.0.lock().unwrap().insert(
            "it".into(),
            Some(WordRuby {
                text: "それは".into(),
                rank: 1,
            }),
        );
        for surface in [
            "It's",
            "“It’s,”",
            "HE'S",
            "don't",
            "I’m",
            "we’re",
            "they've",
            "I'll",
            "I'd",
            "let's",
            "what's",
            "shouldn’t",
        ] {
            assert!(dict.ruby.lookup(&dict, surface).is_none(), "{surface}");
        }
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(dict.ruby.lookup(&dict, "it").unwrap().text, "それは");
        assert_eq!(dict.ruby.lookup(&dict, "thesis’s").unwrap().text, "論文");
    }

    #[test]
    fn bundled_dictionary_ruby_omits_abbreviation_explanations() {
        if std::env::var_os("EJDICT_PATH").is_none() {
            return;
        }
        let dict = DictionaryRegistry::from_env();
        for surface in ["It's", "don't", "gonna", "gotta", "I'm", "we're"] {
            assert!(dict.ruby.lookup(&dict, surface).is_none(), "{surface}");
        }
        assert_eq!(dict.ruby.lookup(&dict, "photo").unwrap().text, "写真");
        assert_eq!(dict.ruby.lookup(&dict, "cello").unwrap().text, "チェロ");
        assert!(!dict.lookup("don't").is_empty());
    }
}
