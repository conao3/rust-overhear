use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::Arc;

use anyhow::{Context, Result, ensure};

use super::{DictEntry, DictSense, Dictionary, Pos, wordnet::WordNet};

pub struct JapaneseWordNet {
    wordnet: Arc<WordNet>,
    lemmas: HashMap<(Pos, u64), Vec<String>>,
}

impl JapaneseWordNet {
    pub fn from_env(wordnet: Arc<WordNet>) -> Result<Self> {
        let path = std::env::var("WORDNET_JA_PATH").context("WORDNET_JA_PATH が未設定")?;
        let file =
            File::open(&path).with_context(|| format!("日本語 WordNet を開けない: {path}"))?;
        Self::read(wordnet, BufReader::new(file))
    }

    fn read(wordnet: Arc<WordNet>, reader: impl BufRead) -> Result<Self> {
        let mut lemmas: HashMap<(Pos, u64), Vec<String>> = HashMap::new();
        for line in reader.lines() {
            let line = line?;
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let fields: Vec<_> = line.split('\t').collect();
            if fields.get(1) != Some(&"jpn:lemma") {
                continue;
            }
            ensure!(fields.len() == 3, "日本語 WordNet の見出し行が不正");
            let (offset, pos) = fields[0]
                .split_once('-')
                .context("日本語 WordNet の synset が不正")?;
            let pos = match pos {
                "n" => Pos::Noun,
                "v" => Pos::Verb,
                "a" | "s" => Pos::Adjective,
                "r" => Pos::Adverb,
                _ => anyhow::bail!("日本語 WordNet の品詞が不正: {pos}"),
            };
            let offset: u64 = offset.parse().context("日本語 WordNet の offset が不正")?;
            let lemma = fields[2].trim().replace('+', "");
            ensure!(!lemma.is_empty(), "日本語 WordNet の見出しが空");
            let translations = lemmas.entry((pos, offset)).or_default();
            if !translations.contains(&lemma) {
                translations.push(lemma);
            }
        }
        ensure!(!lemmas.is_empty(), "日本語 WordNet が空");
        tracing::info!(synsets = lemmas.len(), "日本語 WordNet を読み込んだ");
        Ok(Self { wordnet, lemmas })
    }
}

impl Dictionary for JapaneseWordNet {
    fn id(&self) -> &'static str {
        "wordnet-ja"
    }

    fn display_name(&self) -> &str {
        "日本語 WordNet (英和)"
    }

    fn lookup(&self, surface: &str) -> Vec<DictEntry> {
        self.wordnet
            .lookup_lemmas(surface)
            .into_iter()
            .filter_map(|(lemma, pos)| {
                let senses: Vec<_> = self
                    .wordnet
                    .offsets(pos, &lemma)
                    .iter()
                    .filter_map(|offset| {
                        let translations = self.lemmas.get(&(pos, *offset))?;
                        Some(DictSense {
                            definition: translations[0].clone(),
                            synonyms: translations.iter().skip(1).cloned().collect(),
                            examples: Vec::new(),
                        })
                    })
                    .collect();
                if senses.is_empty() {
                    return None;
                }
                Some(DictEntry {
                    lemma,
                    pos: Some(pos),
                    ipa_us: None,
                    senses,
                    source: self.id().into(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::{DictionaryRegistry, ejdict::EjDict};

    fn english_fixture() -> (tempdir::TempDir, Arc<WordNet>) {
        let dir = tempdir::TempDir::new("wordnet-ja-test").unwrap();
        for (pos, index) in [
            (
                "noun",
                "defibrillation n 1 0 1 0 00698959\ndefibrillator n 1 0 1 0 03171635\n",
            ),
            ("verb", "defibrillate v 1 0 1 0 00698959\n"),
            ("adj", ""),
            ("adv", ""),
        ] {
            std::fs::write(dir.path().join(format!("index.{pos}")), index).unwrap();
        }
        let wordnet = Arc::new(WordNet::open(dir.path()).unwrap());
        (dir, wordnet)
    }

    #[test]
    fn matches_synset_and_pos_and_resolves_inflections() {
        let (_dir, english) = english_fixture();
        let input = "# Japanese Wordnet\n00698959-n\tjpn:lemma\t電気的除細動\n00698959-n\tjpn:lemma\t電気的除細動\n00698959-n\tjpn:lemma\t除細動\n00698959-v\tjpn:lemma\t除細動+する\n03171635-n\tjpn:lemma\t除細動機\n00698959-n\teng:def\t0\tEnglish definition\n";
        let dictionary = JapaneseWordNet::read(english, input.as_bytes()).unwrap();
        let entries = dictionary.lookup("DEFIBRILLATION,");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].lemma, "defibrillation");
        assert_eq!(entries[0].source, "wordnet-ja");
        assert_eq!(entries[0].senses[0].definition, "電気的除細動");
        assert_eq!(entries[0].senses[0].synonyms, ["除細動"]);
        assert_eq!(
            dictionary.lookup("defibrillate")[0].senses[0].definition,
            "除細動する"
        );
        assert_eq!(
            dictionary.lookup("defibrillators")[0].senses[0].definition,
            "除細動機"
        );
        assert!(dictionary.lookup("defiberlation").is_empty());
        let registry = DictionaryRegistry::new(vec![Arc::new(dictionary)]);
        let ruby = registry.ruby.lookup(&registry, "defibrillation").unwrap();
        assert_eq!(ruby.text.as_deref(), Some("電気的除細動"));
        assert_eq!(ruby.rank, 10001);
    }

    #[test]
    fn rejects_invalid_or_empty_japanese_data() {
        let (_dir, english) = english_fixture();
        for data in [
            "# empty\n",
            "bad-n\tjpn:lemma\t語\n",
            "00000001-x\tjpn:lemma\t語\n",
            "00000001-n\tjpn:lemma\t\n",
        ] {
            assert!(JapaneseWordNet::read(english.clone(), data.as_bytes()).is_err());
        }
    }

    #[test]
    fn bundled_dictionaries_translate_medical_terms_and_keep_ejdict_priority() {
        if std::env::var_os("WORDNET_JA_PATH").is_none() {
            return;
        }
        let english = Arc::new(WordNet::from_env().unwrap());
        let japanese = JapaneseWordNet::from_env(english.clone()).unwrap();
        let registry = DictionaryRegistry::new(vec![
            Arc::new(EjDict::from_env().unwrap()),
            Arc::new(japanese),
            english,
        ]);
        for (word, expected) in [
            ("defibrillation", "電気的除細動"),
            ("defibrillator", "除細動機"),
            ("defibrillators", "除細動機"),
        ] {
            let ruby = registry.ruby.lookup(&registry, word).unwrap();
            assert_eq!(ruby.text.as_deref(), Some(expected), "{word}");
            assert_eq!(ruby.rank, 10001);
        }
        let ejdict = DictionaryRegistry::new(vec![Arc::new(EjDict::from_env().unwrap())]);
        for word in ["hear", "dog", "sceptical"] {
            assert_eq!(
                registry.ruby.lookup(&registry, word).unwrap().text,
                ejdict.ruby.lookup(&ejdict, word).unwrap().text
            );
        }
        assert_eq!(
            registry
                .ruby
                .lookup(&registry, "defiberlation")
                .unwrap()
                .text,
            None
        );
    }
}
