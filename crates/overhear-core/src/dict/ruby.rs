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
    /// None means no Japanese translation; the word still has a frequency rank.
    pub text: Option<String>,
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
        if !word.chars().any(|c| c.is_ascii_alphabetic()) || word.len() > 128 {
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
        let mut excluded = false;
        let text = entries
            .iter()
            .filter(|e| matches!(e.source.as_str(), "ejdict" | "wordnet-ja"))
            .flat_map(|e| &e.senses)
            .find_map(|sense| match compact_gloss(&sense.definition) {
                Gloss::Text(text) => Some(text),
                Gloss::Excluded => {
                    excluded = true;
                    None
                }
                Gloss::Missing => None,
            });
        // Explanations of contractions are deliberately omitted, not missing translations.
        let ruby = if text.is_none() && excluded {
            None
        } else {
            Some(WordRuby { text, rank })
        };
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

#[derive(Debug, PartialEq, Eq)]
enum Gloss {
    Text(String),
    Excluded,
    Missing,
}

fn compact_gloss(definition: &str) -> Gloss {
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
        return Gloss::Excluded;
    }
    if !text
        .chars()
        .any(|c| ('\u{3040}'..='\u{30ff}').contains(&c) || ('\u{3400}'..='\u{9fff}').contains(&c))
    {
        return Gloss::Missing;
    }
    let mut short: String = text.chars().take(12).collect();
    if text.chars().count() > 12 {
        short.push('…');
    }
    Gloss::Text(short)
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
            let (lemma, source, definitions) = match word {
                "thesis" => ("thesis", "ejdict", vec!["『論文』,卒業論文"]),
                "gonna" => ("gonna", "ejdict", vec!["going toの縮約形"]),
                "photo" => ("photo", "ejdict", vec!["photographの短縮形", "写真"]),
                "walked" => ("walk", "wordnet", vec!["use one's feet to advance"]),
                _ => return vec![],
            };
            vec![DictEntry {
                lemma: lemma.into(),
                pos: None,
                ipa_us: None,
                source: source.into(),
                senses: definitions
                    .into_iter()
                    .map(|definition| DictSense {
                        definition: definition.into(),
                        synonyms: vec![],
                        examples: vec![],
                    })
                    .collect(),
            }]
        }
    }

    #[test]
    fn caches_normalized_words_and_misses() {
        let calls = Arc::new(AtomicUsize::new(0));
        let dict = DictionaryRegistry::new(vec![Arc::new(CountingDictionary(calls.clone()))]);
        let ruby = dict.ruby.lookup(&dict, "THESIS,").unwrap();
        assert_eq!(ruby.text.as_deref(), Some("論文"));
        assert!(ruby.rank > 1000);
        assert_eq!(dict.ruby.lookup(&dict, "thesis").unwrap(), ruby);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        let missing = dict.ruby.lookup(&dict, "unknownword").unwrap();
        assert_eq!(missing.text, None);
        assert_eq!(missing.rank, 10001);
        assert_eq!(dict.ruby.lookup(&dict, "UNKNOWNWORD!"), Some(missing));
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn missing_translations_keep_surface_and_lemma_frequency_ranks() {
        let dict = DictionaryRegistry::new(vec![Arc::new(CountingDictionary(Arc::default()))]);
        // Even with no dictionary entry, a known frequent word must stay below the threshold.
        let common = dict.ruby.lookup(&dict, "THE,").unwrap();
        assert_eq!(common.text, None);
        assert_eq!(common.rank, FREQUENCIES["the"]);
        assert!(common.rank <= 1000);
        // English-only entries still contribute their base form to the difficulty estimate.
        let inflected = dict.ruby.lookup(&dict, "walked").unwrap();
        assert_eq!(inflected.text, None);
        assert!(inflected.rank <= FREQUENCIES["walk"]);
        let missing = dict.ruby.lookup(&dict, "defiberlation").unwrap();
        assert_eq!(missing.text, None);
        assert_eq!(missing.rank, 10001);
    }

    #[test]
    fn deliberate_exclusions_are_not_marked_as_missing_translations() {
        let calls = Arc::new(AtomicUsize::new(0));
        let dict = DictionaryRegistry::new(vec![Arc::new(CountingDictionary(calls.clone()))]);
        for word in ["", "...", "123", "2026-10", "It's", "don't", "I'm"] {
            assert!(dict.ruby.lookup(&dict, word).is_none(), "{word}");
        }
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert!(dict.ruby.lookup(&dict, "gonna").is_none());
        assert!(dict.ruby.lookup(&dict, "GONNA!").is_none());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        // A real translation in a later sense takes precedence over an excluded explanation.
        assert_eq!(
            dict.ruby.lookup(&dict, "photo").unwrap().text.as_deref(),
            Some("写真")
        );
    }

    #[test]
    fn short_gloss_preserves_japanese_and_omits_usage_labels() {
        assert_eq!(
            compact_gloss("《複数形》『論文』,学位論文"),
            Gloss::Text("論文".into())
        );
        assert_eq!(
            compact_gloss("(物を)聞く;聞こえる"),
            Gloss::Text("聞く".into())
        );
        assert_eq!(compact_gloss("= example"), Gloss::Missing);
        assert_eq!(
            compact_gloss("あいうえおかきくけこさしすせそ"),
            Gloss::Text("あいうえおかきくけこさし…".into())
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
            assert_eq!(compact_gloss(definition), Gloss::Excluded, "{definition}");
        }
        assert_eq!(
            compact_gloss("『写真』[photographの短縮形]"),
            Gloss::Text("写真".into())
        );
        assert_eq!(
            compact_gloss("チェロ(楽器)[violoncelloの短縮形]"),
            Gloss::Text("チェロ".into())
        );
        assert_eq!(compact_gloss("省略形"), Gloss::Text("省略形".into()));
        assert_eq!(
            compact_gloss("戦争の略奪品"),
            Gloss::Text("戦争の略奪品".into())
        );
    }

    #[test]
    fn contractions_are_filtered_before_possessive_normalization_and_cache_lookup() {
        let calls = Arc::new(AtomicUsize::new(0));
        let dict = DictionaryRegistry::new(vec![Arc::new(CountingDictionary(calls.clone()))]);
        dict.ruby.0.lock().unwrap().insert(
            "it".into(),
            Some(WordRuby {
                text: Some("それは".into()),
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
        assert_eq!(
            dict.ruby.lookup(&dict, "it").unwrap().text.as_deref(),
            Some("それは")
        );
        assert_eq!(
            dict.ruby.lookup(&dict, "thesis’s").unwrap().text.as_deref(),
            Some("論文")
        );
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
        assert_eq!(
            dict.ruby.lookup(&dict, "photo").unwrap().text.as_deref(),
            Some("写真")
        );
        assert_eq!(
            dict.ruby.lookup(&dict, "cello").unwrap().text.as_deref(),
            Some("チェロ")
        );
        assert!(!dict.lookup("don't").is_empty());
    }
}
