//! 翻訳 API のキーの保存先。
//!
//! Secret Service (gnome-keyring 等) に `overhear` のサービス名で置く。
//! 起動時に読み込んで持っておき、翻訳のたびに D-Bus を叩かない。

use std::collections::HashMap;
use std::sync::RwLock;

use anyhow::{Context, Result};

const SERVICE: &str = "overhear";

/// キーの実体の置き場所。テストではメモリに差し替える。
pub trait KeyBackend: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>>;
    fn set(&self, account: &str, key: &str) -> Result<()>;
    fn delete(&self, account: &str) -> Result<()>;
}

/// Secret Service。
pub struct KeyringBackend;

impl KeyBackend for KeyringBackend {
    fn get(&self, account: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new(SERVICE, account)?;
        match entry.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(err).with_context(|| format!("{account} のキーを読めない")),
        }
    }

    fn set(&self, account: &str, key: &str) -> Result<()> {
        keyring::Entry::new(SERVICE, account)?
            .set_password(key)
            .with_context(|| format!("{account} のキーを保存できない"))
    }

    fn delete(&self, account: &str) -> Result<()> {
        match keyring::Entry::new(SERVICE, account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err).with_context(|| format!("{account} のキーを消せない")),
        }
    }
}

pub struct ApiKeys {
    backend: Box<dyn KeyBackend>,
    cache: RwLock<HashMap<String, String>>,
}

impl ApiKeys {
    /// `accounts` のキーを読み込んでおく。
    pub fn load(backend: Box<dyn KeyBackend>, accounts: &[&str]) -> Result<Self> {
        let mut cache = HashMap::new();
        for account in accounts {
            if let Some(key) = backend.get(account)? {
                cache.insert(account.to_string(), key);
            }
        }
        Ok(Self {
            backend,
            cache: RwLock::new(cache),
        })
    }

    pub fn get(&self, account: &str) -> Option<String> {
        self.cache.read().ok()?.get(account).cloned()
    }

    /// キーを保存する。`None` か空文字で消す。
    pub fn set(&self, account: &str, key: Option<&str>) -> Result<()> {
        let key = key.map(str::trim).filter(|k| !k.is_empty());
        match key {
            Some(key) => self.backend.set(account, key)?,
            None => self.backend.delete(account)?,
        }
        let mut cache = self
            .cache
            .write()
            .map_err(|_| anyhow::anyhow!("lock poisoned"))?;
        match key {
            Some(key) => cache.insert(account.to_string(), key.to_string()),
            None => cache.remove(account),
        };
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct MemoryBackend(pub Mutex<HashMap<String, String>>);

    impl KeyBackend for MemoryBackend {
        fn get(&self, account: &str) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(account).cloned())
        }
        fn set(&self, account: &str, key: &str) -> Result<()> {
            self.0.lock().unwrap().insert(account.into(), key.into());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<()> {
            self.0.lock().unwrap().remove(account);
            Ok(())
        }
    }

    #[test]
    fn loads_sets_and_deletes() {
        let backend = MemoryBackend::default();
        backend.set("deepl", "k1").unwrap();
        let keys = ApiKeys::load(Box::new(backend), &["deepl", "google"]).unwrap();
        assert_eq!(keys.get("deepl").as_deref(), Some("k1"));
        assert_eq!(keys.get("google"), None);

        keys.set("google", Some("  k2 ")).unwrap();
        assert_eq!(keys.get("google").as_deref(), Some("k2"));
        keys.set("deepl", Some("")).unwrap();
        assert_eq!(keys.get("deepl"), None);
    }
}
