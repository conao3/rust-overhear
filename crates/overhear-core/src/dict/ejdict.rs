//! ejdict-hand による英和辞書。
//!
//! パブリックドメインのタブ区切りテキスト (`見出し<TAB>語義 / 語義 / …`)
//! をそのまま読む。4.5 万見出しで 4MB 程度なので、起動時に全部メモリへ
//! 載せてしまう。flake の devShell が `EJDICT_PATH` で指す。
//!
//! 品詞情報は持たないため `DictEntry::pos` は None。活用の解決も
//! 裏付けが無いので、候補を出して辞書に存在するものだけを採る。

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result, anyhow};

use super::{DictEntry, DictSense, Dictionary, inflection_candidates, normalize_surface};

/// 語義の区切り。
const SENSE_SEPARATOR: &str = " / ";

pub struct EjDict {
    entries: HashMap<String, Vec<String>>,
}

impl EjDict {
    pub fn from_env() -> Result<Self> {
        let path = std::env::var("EJDICT_PATH")
            .context("EJDICT_PATH が未設定 (nix develop の外で実行していないか)")?;
        Self::open(path)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let file =
            File::open(path).with_context(|| format!("英和辞書を開けない: {}", path.display()))?;

        let mut entries: HashMap<String, Vec<String>> = HashMap::new();
        for line in BufReader::new(file).lines() {
            let line = line?;
            let Some((word, meaning)) = line.split_once('\t') else {
                continue;
            };
            let word = word.trim().to_lowercase();
            if word.is_empty() {
                continue;
            }
            let senses: Vec<String> = meaning
                .split(SENSE_SEPARATOR)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !senses.is_empty() {
                entries.entry(word).or_default().extend(senses);
            }
        }

        if entries.is_empty() {
            return Err(anyhow!("英和辞書が空: {}", path.display()));
        }
        tracing::info!(lemmas = entries.len(), path = %path.display(), "英和辞書を読み込んだ");
        Ok(Self { entries })
    }

    fn senses_for(&self, word: &str) -> Vec<DictSense> {
        let Some(senses) = self.entries.get(word) else {
            return Vec::new();
        };
        let mut pending: Vec<_> = senses.iter().rev().collect();
        let mut visited = HashSet::from([word.to_owned()]);
        let mut resolved = Vec::new();
        while let Some(definition) = pending.pop() {
            resolved.push(DictSense {
                definition: definition.clone(),
                synonyms: Vec::new(),
                examples: Vec::new(),
            });
            if let Some(reference) = definition.strip_prefix('=') {
                let target = reference.trim().to_lowercase();
                if visited.insert(target.clone()) {
                    if let Some(senses) = self.entries.get(&target) {
                        pending.extend(senses.iter().rev());
                    }
                }
            }
        }
        resolved
    }
}

impl Dictionary for EjDict {
    fn id(&self) -> &'static str {
        "ejdict"
    }

    fn display_name(&self) -> &str {
        "ejdict-hand (英和)"
    }

    fn lookup(&self, surface: &str) -> Vec<DictEntry> {
        let cleaned = normalize_surface(surface);
        if cleaned.is_empty() {
            return Vec::new();
        }

        // 候補の先頭 (表層形そのもの) が引ければそれを優先し、
        // 無ければ活用を解いた候補を順に試す。最初に当たった 1 件だけ返す。
        for candidate in inflection_candidates(&cleaned) {
            if self.entries.contains_key(&candidate) {
                return vec![DictEntry {
                    senses: self.senses_for(&candidate),
                    lemma: candidate,
                    pos: None,
                    ipa_us: None,
                    source: "ejdict".to_string(),
                }];
            }
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict() -> Option<EjDict> {
        EjDict::from_env().ok()
    }

    #[test]
    fn looks_up_a_plain_word() {
        let Some(d) = dict() else {
            eprintln!("EJDICT_PATH が無いので skip");
            return;
        };
        let entries = d.lookup("hear");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].lemma, "hear");
        assert!(entries[0].pos.is_none(), "英和は品詞を持たない");
        assert!(
            entries[0]
                .senses
                .iter()
                .any(|s| s.definition.contains("聞")),
            "{:?}",
            entries[0].senses.first()
        );
    }

    #[test]
    fn resolves_simple_inflections() {
        let Some(d) = dict() else { return };
        assert_eq!(d.lookup("dogs")[0].lemma, "dog");
        assert_eq!(d.lookup("studies")[0].lemma, "study");
        // 字幕のトークンは句読点つきで来る
        assert_eq!(d.lookup("HEARD,")[0].lemma, "heard");
    }

    #[test]
    fn unknown_word_returns_empty() {
        let Some(d) = dict() else { return };
        assert!(d.lookup("zzzqqxyz").is_empty());
    }

    #[test]
    fn ruby_resolves_references_and_keeps_the_original_headword() {
        use std::sync::Arc;

        let dictionary = EjDict {
            entries: HashMap::from([
                ("sceptical".into(), vec!["=skeptical".into()]),
                ("skeptical".into(), vec!["疑い深い,懐疑的な".into()]),
                ("alias".into(), vec!["=sceptical".into()]),
                ("a-bomb".into(), vec!["=atomic bomb".into()]),
                ("atomic bomb".into(), vec!["原子爆弾".into()]),
            ]),
        };
        assert_eq!(dictionary.lookup("sceptical")[0].lemma, "sceptical");
        let registry = super::super::DictionaryRegistry::new(vec![Arc::new(dictionary)]);
        for word in ["sceptical", "alias"] {
            assert_eq!(
                registry
                    .ruby
                    .lookup(&registry, word)
                    .unwrap()
                    .text
                    .as_deref(),
                Some("疑い深い")
            );
        }
        assert_eq!(
            registry
                .ruby
                .lookup(&registry, "A-BOMB")
                .unwrap()
                .text
                .as_deref(),
            Some("原子爆弾")
        );
    }

    #[test]
    fn references_terminate_on_cycles_and_preserve_other_senses() {
        let dictionary = EjDict {
            entries: HashMap::from([
                ("one".into(), vec!["=two".into(), "一つ".into()]),
                ("two".into(), vec!["=one".into(), "二つ".into()]),
                ("missing".into(), vec!["=absent".into(), "不足".into()]),
            ]),
        };
        let entries = dictionary.lookup("one");
        let definitions: Vec<_> = entries[0]
            .senses
            .iter()
            .map(|s| s.definition.as_str())
            .collect();
        assert_eq!(definitions, ["=two", "=one", "二つ", "一つ"]);
        assert_eq!(dictionary.lookup("missing")[0].senses[1].definition, "不足");
    }

    #[test]
    fn resolves_references_in_the_bundled_dictionary() {
        use std::sync::Arc;

        let Some(dictionary) = dict() else { return };
        let registry = super::super::DictionaryRegistry::new(vec![Arc::new(dictionary)]);
        for (word, expected) in [
            ("sceptical", "疑い深い"),
            ("admonishment", "忠告"),
            ("aeroplane", "飛行機"),
            ("A-BOMB", "原子爆弾"),
        ] {
            assert_eq!(
                registry
                    .ruby
                    .lookup(&registry, word)
                    .unwrap()
                    .text
                    .as_deref(),
                Some(expected),
                "{word}"
            );
        }
    }
}
