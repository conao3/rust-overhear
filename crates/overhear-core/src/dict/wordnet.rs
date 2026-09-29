//! WordNet 3.0 の dict ファイルを直接読む英英辞書。
//!
//! nixpkgs の `wordnet` パッケージに `dict/` が入っているため、追加の
//! ダウンロードは要らない。flake の devShell が `WORDNET_DICT_DIR` で指す。
//!
//! index は起動時にメモリへ載せ (約 15 万見出し)、語義は data ファイルの
//! 該当オフセットを seek して読む。

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use super::{DictEntry, DictSense, Dictionary, Pos};

fn pos_suffix(pos: Pos) -> &'static str {
    match pos {
        Pos::Noun => "noun",
        Pos::Verb => "verb",
        Pos::Adjective => "adj",
        Pos::Adverb => "adv",
    }
}

/// WordNet の morphy が使う detachment rules。(接尾辞, 置換後)。
fn detachment_rules(pos: Pos) -> &'static [(&'static str, &'static str)] {
    match pos {
        Pos::Noun => &[
            ("ses", "s"),
            ("xes", "x"),
            ("zes", "z"),
            ("ches", "ch"),
            ("shes", "sh"),
            ("men", "man"),
            ("ies", "y"),
            ("s", ""),
        ],
        Pos::Verb => &[
            ("ies", "y"),
            ("es", "e"),
            ("es", ""),
            ("ed", "e"),
            ("ed", ""),
            ("ing", "e"),
            ("ing", ""),
            ("s", ""),
        ],
        Pos::Adjective => &[("er", ""), ("est", ""), ("er", "e"), ("est", "e")],
        Pos::Adverb => &[],
    }
}

struct PosData {
    /// 見出し語 -> synset のオフセット列。
    index: HashMap<String, Vec<u64>>,
    /// 活用形 -> 原形。
    exceptions: HashMap<String, Vec<String>>,
    data_path: PathBuf,
}

pub struct WordNet {
    by_pos: HashMap<Pos, PosData>,
}

impl WordNet {
    pub fn from_env() -> Result<Self> {
        let dir = std::env::var("WORDNET_DICT_DIR")
            .context("WORDNET_DICT_DIR が未設定 (nix develop の外で実行していないか)")?;
        Self::open(dir)
    }

    pub fn open(dict_dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dict_dir.as_ref();
        if !dir.join("index.noun").exists() {
            return Err(anyhow!(
                "WordNet の dict ディレクトリが見つからない: {}",
                dir.display()
            ));
        }

        let mut by_pos = HashMap::new();
        for pos in Pos::all() {
            let suffix = pos_suffix(pos);
            let index = load_index(&dir.join(format!("index.{suffix}")))
                .with_context(|| format!("index.{suffix} の読み込み"))?;
            let exceptions =
                load_exceptions(&dir.join(format!("{suffix}.exc"))).unwrap_or_default();
            by_pos.insert(
                pos,
                PosData {
                    index,
                    exceptions,
                    data_path: dir.join(format!("data.{suffix}")),
                },
            );
        }

        let total: usize = by_pos.values().map(|p| p.index.len()).sum();
        tracing::info!(lemmas = total, dir = %dir.display(), "WordNet を読み込んだ");
        Ok(Self { by_pos })
    }

    /// 活用を解いて、index に存在する見出し語の候補を返す。
    fn morphy(&self, word: &str, pos: Pos) -> Vec<String> {
        let Some(data) = self.by_pos.get(&pos) else {
            return Vec::new();
        };
        let word = word.to_lowercase();
        let mut out: Vec<String> = Vec::new();

        // 例外リストが最優先。
        if let Some(bases) = data.exceptions.get(&word) {
            for base in bases {
                if data.index.contains_key(base) && !out.contains(base) {
                    out.push(base.clone());
                }
            }
        }
        if data.index.contains_key(&word) && !out.contains(&word) {
            out.push(word.clone());
        }
        for (suffix, replacement) in detachment_rules(pos) {
            if let Some(stem) = word.strip_suffix(suffix) {
                let candidate = format!("{stem}{replacement}");
                if !candidate.is_empty()
                    && data.index.contains_key(&candidate)
                    && !out.contains(&candidate)
                {
                    out.push(candidate);
                }
            }
        }
        out
    }

    fn senses_for(&self, pos: Pos, lemma: &str) -> Vec<DictSense> {
        let Some(data) = self.by_pos.get(&pos) else {
            return Vec::new();
        };
        let Some(offsets) = data.index.get(lemma) else {
            return Vec::new();
        };
        let Ok(mut file) = File::open(&data.data_path) else {
            return Vec::new();
        };
        offsets
            .iter()
            .filter_map(|offset| read_synset(&mut file, *offset, lemma))
            .collect()
    }
}

impl Dictionary for WordNet {
    fn id(&self) -> &'static str {
        "wordnet"
    }

    fn display_name(&self) -> &str {
        "WordNet 3.0 (英英)"
    }

    fn lookup(&self, surface: &str) -> Vec<DictEntry> {
        let cleaned = super::normalize_surface(surface);
        if cleaned.is_empty() {
            return Vec::new();
        }

        let mut entries = Vec::new();
        for pos in Pos::all() {
            for lemma in self.morphy(&cleaned, pos) {
                let senses = self.senses_for(pos, &lemma);
                if !senses.is_empty() {
                    entries.push(DictEntry {
                        lemma,
                        pos: Some(pos),
                        senses,
                        source: "wordnet".to_string(),
                    });
                }
            }
        }
        entries
    }
}

/// `index.<pos>` を読む。
///
/// 形式: `lemma pos synset_cnt p_cnt [ptr_symbol...] sense_cnt tagsense_cnt offset...`
fn load_index(path: &Path) -> Result<HashMap<String, Vec<u64>>> {
    let file = File::open(path)?;
    let mut map = HashMap::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        // 冒頭のライセンス行は空白 2 つで始まる。
        if line.starts_with("  ") || line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 6 {
            continue;
        }
        let lemma = fields[0].to_string();
        let Ok(synset_cnt) = fields[2].parse::<usize>() else {
            continue;
        };
        let Ok(p_cnt) = fields[3].parse::<usize>() else {
            continue;
        };
        // ptr_symbol を読み飛ばし、sense_cnt / tagsense_cnt の次からがオフセット。
        let offsets_at = 4 + p_cnt + 2;
        if fields.len() < offsets_at + synset_cnt {
            continue;
        }
        let offsets: Vec<u64> = fields[offsets_at..offsets_at + synset_cnt]
            .iter()
            .filter_map(|s| s.parse::<u64>().ok())
            .collect();
        map.insert(lemma, offsets);
    }
    Ok(map)
}

/// `<pos>.exc` を読む。形式: `inflected base [base...]`
fn load_exceptions(path: &Path) -> Result<HashMap<String, Vec<String>>> {
    let file = File::open(path)?;
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        let mut fields = line.split_whitespace();
        let Some(inflected) = fields.next() else {
            continue;
        };
        let bases: Vec<String> = fields.map(str::to_string).collect();
        if !bases.is_empty() {
            map.insert(inflected.to_string(), bases);
        }
    }
    Ok(map)
}

/// `data.<pos>` の 1 synset を読む。
///
/// 形式: `offset lex_filenum ss_type w_cnt(hex) word lex_id ... | gloss`
fn read_synset(file: &mut File, offset: u64, self_lemma: &str) -> Option<DictSense> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut reader = BufReader::new(file.try_clone().ok()?);
    let mut line = String::new();
    // 行の長さは高々数 KB。
    let mut buf = Vec::new();
    reader.by_ref().take(8192).read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    line.push_str(text.lines().next()?);

    let (left, gloss) = line.split_once('|')?;
    let fields: Vec<&str> = left.split_whitespace().collect();
    if fields.len() < 4 {
        return None;
    }
    let w_cnt = usize::from_str_radix(fields[3], 16).ok()?;

    let mut synonyms = Vec::new();
    for i in 0..w_cnt {
        let at = 4 + i * 2;
        if at >= fields.len() {
            break;
        }
        // WordNet は語中の空白を _ で表し、形容詞は末尾に (a) 等の marker を付ける。
        let word = fields[at]
            .replace('_', " ")
            .split('(')
            .next()
            .unwrap_or_default()
            .to_string();
        if !word.is_empty() && !word.eq_ignore_ascii_case(self_lemma) {
            synonyms.push(word);
        }
    }

    // gloss は `定義; 定義; "例文"; "例文"` の形。
    let mut definition = Vec::new();
    let mut examples = Vec::new();
    for part in gloss.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part.starts_with('"') {
            examples.push(part.trim_matches('"').trim().to_string());
        } else {
            definition.push(part.to_string());
        }
    }

    Some(DictSense {
        definition: definition.join("; "),
        synonyms,
        examples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wordnet() -> Option<WordNet> {
        WordNet::from_env().ok()
    }

    #[test]
    fn looks_up_a_plain_noun() {
        let Some(wn) = wordnet() else {
            eprintln!("WORDNET_DICT_DIR が無いので skip");
            return;
        };
        let entries = wn.lookup("dog");
        assert!(!entries.is_empty(), "dog が引けない");
        let noun = entries.iter().find(|e| e.pos == Some(Pos::Noun)).unwrap();
        assert_eq!(noun.lemma, "dog");
        assert!(noun.senses[0].definition.contains("domesticated"));
    }

    #[test]
    fn resolves_inflections() {
        let Some(wn) = wordnet() else { return };
        // 複数形・過去形・例外リストのいずれも原形に解ける
        assert_eq!(wn.lookup("dogs")[0].lemma, "dog");
        assert!(wn.lookup("running").iter().any(|e| e.lemma == "run"));
        assert!(wn.lookup("children").iter().any(|e| e.lemma == "child"));
    }

    #[test]
    fn strips_punctuation_and_possessive() {
        let Some(wn) = wordnet() else { return };
        // 字幕のトークンは "WELL," のように句読点がくっついてくる
        assert!(wn.lookup("DOG,").iter().any(|e| e.lemma == "dog"));
        assert!(wn.lookup("dog's").iter().any(|e| e.lemma == "dog"));
    }

    #[test]
    fn unknown_word_returns_empty() {
        let Some(wn) = wordnet() else { return };
        assert!(wn.lookup("zzzqqxyz").is_empty());
    }
}
