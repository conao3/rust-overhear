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
        let current = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("{} を読めない", path.display()))?;
            serde_json::from_str(&text)
                .with_context(|| format!("{} を解釈できない", path.display()))?
        } else {
            Settings::default()
        };
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
        change(&mut current);
        let text = serde_json::to_string_pretty(&*current)?;
        // 書きかけのファイルを残さないよう、一時ファイルから置き換える。
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text + "\n")
            .with_context(|| format!("{} に書けない", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("{} に書けない", self.path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_across_reopen() {
        let dir = tempdir::TempDir::new("overhear-settings").unwrap();
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
    fn rejects_a_broken_file() {
        let dir = tempdir::TempDir::new("overhear-settings").unwrap();
        std::fs::write(dir.path().join("settings.json"), "{ not json").unwrap();
        assert!(SettingsStore::open(dir.path()).is_err());
    }
}
