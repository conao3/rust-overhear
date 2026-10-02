use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

use anyhow::{Context, Result, ensure};

#[derive(Default)]
pub struct IpaDict {
    entries: HashMap<String, String>,
}

impl IpaDict {
    pub fn from_env() -> Result<Self> {
        let path = std::env::var("IPA_US_PATH").context("IPA_US_PATH が未設定")?;
        let file = File::open(&path).with_context(|| format!("発音辞書を開けない: {path}"))?;
        let dict = Self::read(BufReader::new(file))?;
        tracing::info!(lemmas = dict.entries.len(), "米語の発音辞書を読み込んだ");
        Ok(dict)
    }

    pub(super) fn read(reader: impl BufRead) -> Result<Self> {
        let mut entries = HashMap::new();
        for line in reader.lines() {
            let line = line?;
            let Some((word, ipa)) = line.split_once('\t') else {
                continue;
            };
            let word = word.trim().to_lowercase();
            let ipa = ipa.trim();
            if !word.is_empty() && !ipa.is_empty() {
                entries.insert(word, ipa.to_owned());
            }
        }
        ensure!(!entries.is_empty(), "発音辞書が空");
        Ok(Self { entries })
    }

    pub fn lookup(&self, lemma: &str) -> Option<&str> {
        self.entries.get(&lemma.to_lowercase()).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_ipa_stress_and_alternative_pronunciations() {
        let dict = IpaDict::read(
            "hello\t/həˈɫoʊ/, /hɛˈɫoʊ/\r\nread\t/ˈɹɛd/, /ˈɹid/\ninvalid\n\t/foo/\nempty\t  \n"
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(dict.lookup("HELLO"), Some("/həˈɫoʊ/, /hɛˈɫoʊ/"));
        assert_eq!(dict.lookup("read"), Some("/ˈɹɛd/, /ˈɹid/"));
        assert_eq!(dict.lookup("unknown"), None);
        assert_eq!(dict.lookup("empty"), None);
        assert_eq!(dict.lookup(""), None);
    }

    #[test]
    fn rejects_empty_data_and_propagates_read_errors() {
        assert!(IpaDict::read(&b"invalid\n\t\n"[..]).is_err());
        assert!(IpaDict::read(&b"valid\t/ipa/\n\xff"[..]).is_err());
        assert_eq!(IpaDict::default().lookup("hello"), None);
    }
}
