//! UI から変えた設定の永続化。
//!
//! 字幕や履歴と違い、選んだ翻訳エンジンや音源は次の起動でも同じであって
//! ほしい。データディレクトリの `settings.json` に置く。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// 既定の翻訳エンジン。
    pub translator: Option<String>,
    /// 拾うノード。None なら既定シンク。
    pub capture_target: Option<String>,
    /// 訳す先の言語 (`ja` 等)。
    pub target_lang: Option<String>,
    /// Ollama で使うモデル。
    pub ollama_model: Option<String>,
    pub refinement_enabled: bool,
    pub ruby: RubySettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RubySettings {
    pub enabled: bool,
    pub known_words: i32,
    pub size: i32,
}

impl Default for RubySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            known_words: 1000,
            size: 75,
        }
    }
}

impl RubySettings {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            (0..=10000).contains(&self.known_words),
            "単語レベルは 0〜10000 の範囲で指定してください"
        );
        anyhow::ensure!(
            (40..=150).contains(&self.size),
            "ルビのサイズは 40〜150% の範囲で指定してください"
        );
        Ok(())
    }
}

pub struct SettingsStore {
    path: PathBuf,
    current: Mutex<Settings>,
}

impl SettingsStore {
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self> {
        let data_dir = data_dir.as_ref();
        std::fs::create_dir_all(data_dir).context("データディレクトリの作成")?;
        let path = data_dir.join("settings.json");
        let current: Settings = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("{} を読めない", path.display()))?;
            serde_json::from_str(&text)
                .with_context(|| format!("{} を解釈できない", path.display()))?
        } else {
            Settings::default()
        };
        current.ruby.validate()?;
        Ok(Self {
            path,
            current: Mutex::new(current),
        })
    }

    pub fn get(&self) -> Settings {
        self.current.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// 設定を書き換えて保存する。
    pub fn update(&self, change: impl FnOnce(&mut Settings)) -> Result<()> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| anyhow::anyhow!("lock poisoned"))?;
        let mut updated = current.clone();
        change(&mut updated);
        updated.ruby.validate()?;
        let text = serde_json::to_string_pretty(&updated)?;
        // 書きかけのファイルを残さないよう、一時ファイルから置き換える。
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text + "\n")
            .with_context(|| format!("{} に書けない", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("{} に書けない", self.path.display()))?;
        *current = updated;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_across_reopen() {
        let dir = tempfile::Builder::new()
            .prefix("overhear-settings")
            .tempdir()
            .unwrap();
        let store = SettingsStore::open(dir.path()).unwrap();
        assert_eq!(store.get(), Settings::default());
        store
            .update(|s| {
                s.translator = Some("deepl".into());
                s.capture_target = Some("alsa_output.usb".into());
            })
            .unwrap();

        let reopened = SettingsStore::open(dir.path()).unwrap();
        assert_eq!(reopened.get().translator.as_deref(), Some("deepl"));
        assert_eq!(
            reopened.get().capture_target.as_deref(),
            Some("alsa_output.usb")
        );
    }

    #[test]
    fn ruby_settings_persist_and_reject_invalid_ranges() {
        let dir = tempfile::Builder::new()
            .prefix("overhear-ruby-settings")
            .tempdir()
            .unwrap();
        std::fs::write(dir.path().join("settings.json"), r#"{"translator":"none"}"#).unwrap();
        let store = SettingsStore::open(dir.path()).unwrap();
        assert_eq!(store.get().ruby, RubySettings::default());
        let ruby = RubySettings {
            enabled: false,
            known_words: 5000,
            size: 110,
        };
        store.update(|s| s.ruby = ruby).unwrap();
        assert_eq!(SettingsStore::open(dir.path()).unwrap().get().ruby, ruby);
        for size in [39, 151] {
            assert!(store.update(|s| s.ruby.size = size).is_err());
        }
        for known_words in [-1, 10001] {
            assert!(store.update(|s| s.ruby.known_words = known_words).is_err());
        }
        assert_eq!(store.get().ruby, ruby);
        assert_eq!(SettingsStore::open(dir.path()).unwrap().get().ruby, ruby);
    }

    #[test]
    fn rejects_a_broken_file() {
        let dir = tempfile::Builder::new()
            .prefix("overhear-settings")
            .tempdir()
            .unwrap();
        std::fs::write(dir.path().join("settings.json"), "{ not json").unwrap();
        assert!(SettingsStore::open(dir.path()).is_err());
    }

    #[test]
    fn refinement_defaults_off_and_persists_both_choices() {
        let dir = tempfile::Builder::new()
            .prefix("overhear-refinement")
            .tempdir()
            .unwrap();
        std::fs::write(dir.path().join("settings.json"), r#"{"translator":"none"}"#).unwrap();
        let store = SettingsStore::open(dir.path()).unwrap();
        assert!(!store.get().refinement_enabled);
        for enabled in [true, false] {
            store.update(|s| s.refinement_enabled = enabled).unwrap();
            let reopened = SettingsStore::open(dir.path()).unwrap();
            assert_eq!(reopened.get().refinement_enabled, enabled);
            assert_eq!(reopened.get().translator.as_deref(), Some("none"));
        }
    }

    #[test]
    fn failed_save_preserves_current_settings() {
        let dir = tempfile::Builder::new()
            .prefix("overhear-settings-failure")
            .tempdir()
            .unwrap();
        let store = SettingsStore::open(dir.path()).unwrap();
        std::fs::create_dir(dir.path().join("settings.json.tmp")).unwrap();
        assert!(store.update(|s| s.refinement_enabled = true).is_err());
        assert!(!store.get().refinement_enabled);
    }
}
