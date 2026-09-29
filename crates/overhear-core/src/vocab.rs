//! 語彙ストア。
//!
//! 字幕そのものは流れて消える揮発データだが、**保存した語だけは残す**。
//! 保存時にその文・訳・語義・音声を一緒に焼き付けるので、後から元の
//! segment やリングバッファが消えていても復元できる。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VocabItem {
    pub id: i64,
    pub lemma: String,
    pub surface: String,
    /// 保存した時点の文。元の segment が消えても読める。
    pub sentence: String,
    pub translation: Option<String>,
    pub definition: Option<String>,
    /// 焼き付けた音声 (WAV) の絶対パス。
    pub audio_path: Option<String>,
    pub created_at: String,
    pub anki_note_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct NewVocab {
    pub lemma: String,
    pub surface: String,
    pub sentence: String,
    pub translation: Option<String>,
    pub definition: Option<String>,
    pub audio: Option<Vec<i16>>,
    pub sample_rate: u32,
}

pub struct VocabStore {
    conn: Mutex<Connection>,
    data_dir: PathBuf,
}

impl VocabStore {
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(data_dir.join("audio")).context("データディレクトリの作成")?;
        let conn = Connection::open(data_dir.join("db.sqlite")).context("SQLite を開けない")?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            CREATE TABLE IF NOT EXISTS vocab (
              id           INTEGER PRIMARY KEY AUTOINCREMENT,
              lemma        TEXT NOT NULL,
              surface      TEXT NOT NULL,
              sentence     TEXT NOT NULL,
              translation  TEXT,
              definition   TEXT,
              audio_path   TEXT,
              created_at   TEXT NOT NULL,
              anki_note_id INTEGER
            );
            CREATE INDEX IF NOT EXISTS vocab_lemma_idx ON vocab(lemma);
            "#,
        )
        .context("スキーマの作成")?;

        tracing::info!(dir = %data_dir.display(), "語彙ストアを開いた");
        Ok(Self {
            conn: Mutex::new(conn),
            data_dir,
        })
    }

    pub fn save(&self, item: NewVocab) -> Result<VocabItem> {
        let created_at = chrono::Local::now().to_rfc3339();

        // 音声は保存時点で焼き付ける。リングバッファから溢れても残る。
        let audio_path = match &item.audio {
            Some(samples) if !samples.is_empty() => {
                let name = format!(
                    "{}-{}.wav",
                    chrono::Local::now().format("%Y%m%d-%H%M%S%.3f"),
                    sanitize(&item.lemma)
                );
                let path = self.data_dir.join("audio").join(name);
                std::fs::write(&path, crate::ring::encode_wav(samples, item.sample_rate))
                    .context("音声の書き出し")?;
                Some(path.to_string_lossy().into_owned())
            }
            _ => None,
        };

        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("lock poisoned"))?;
        conn.execute(
            "INSERT INTO vocab (lemma, surface, sentence, translation, definition, audio_path, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                item.lemma,
                item.surface,
                item.sentence,
                item.translation,
                item.definition,
                audio_path,
                created_at,
            ],
        )?;
        let id = conn.last_insert_rowid();

        Ok(VocabItem {
            id,
            lemma: item.lemma,
            surface: item.surface,
            sentence: item.sentence,
            translation: item.translation,
            definition: item.definition,
            audio_path,
            created_at,
            anki_note_id: None,
        })
    }

    pub fn list(&self, limit: usize, offset: usize) -> Result<Vec<VocabItem>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("lock poisoned"))?;
        let mut stmt = conn.prepare(
            "SELECT id, lemma, surface, sentence, translation, definition, audio_path, created_at, anki_note_id
             FROM vocab ORDER BY id DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![limit as i64, offset as i64], |row| {
            Ok(VocabItem {
                id: row.get(0)?,
                lemma: row.get(1)?,
                surface: row.get(2)?,
                sentence: row.get(3)?,
                translation: row.get(4)?,
                definition: row.get(5)?,
                audio_path: row.get(6)?,
                created_at: row.get(7)?,
                anki_note_id: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get(&self, id: i64) -> Result<Option<VocabItem>> {
        Ok(self
            .list(usize::MAX.min(10_000), 0)?
            .into_iter()
            .find(|v| v.id == id))
    }

    pub fn remove(&self, id: i64) -> Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("lock poisoned"))?;
        // 焼き付けた音声も一緒に消す。
        if let Ok(Some(path)) = conn.query_row(
            "SELECT audio_path FROM vocab WHERE id = ?1",
            params![id],
            |row| row.get::<_, Option<String>>(0),
        ) {
            let _ = std::fs::remove_file(path);
        }
        Ok(conn.execute("DELETE FROM vocab WHERE id = ?1", params![id])? > 0)
    }

    pub fn mark_anki(&self, id: i64, note_id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("lock poisoned"))?;
        conn.execute(
            "UPDATE vocab SET anki_note_id = ?1 WHERE id = ?2",
            params![note_id, id],
        )?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("lock poisoned"))?;
        Ok(conn.query_row("SELECT COUNT(*) FROM vocab", [], |row| row.get(0))?)
    }
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(24)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (VocabStore, tempdir::TempDir) {
        let dir = tempdir::TempDir::new("overhear-vocab").unwrap();
        (VocabStore::open(dir.path()).unwrap(), dir)
    }

    fn sample(lemma: &str) -> NewVocab {
        NewVocab {
            lemma: lemma.to_string(),
            surface: lemma.to_uppercase(),
            sentence: format!("THIS IS A {} SENTENCE.", lemma.to_uppercase()),
            translation: Some("これはテストの文です。".to_string()),
            definition: Some("a test".to_string()),
            audio: Some(vec![0i16; 1600]),
            sample_rate: 16_000,
        }
    }

    #[test]
    fn saves_and_lists() {
        let (store, _dir) = store();
        let saved = store.save(sample("dog")).unwrap();
        assert_eq!(saved.lemma, "dog");
        // 音声が焼き付けられている
        let path = saved.audio_path.clone().unwrap();
        assert!(std::path::Path::new(&path).exists());

        store.save(sample("cat")).unwrap();
        let list = store.list(10, 0).unwrap();
        assert_eq!(list.len(), 2);
        // 新しいものが先頭
        assert_eq!(list[0].lemma, "cat");
        assert_eq!(store.count().unwrap(), 2);
    }

    #[test]
    fn removes_with_audio() {
        let (store, _dir) = store();
        let saved = store.save(sample("dog")).unwrap();
        let path = saved.audio_path.clone().unwrap();
        assert!(store.remove(saved.id).unwrap());
        assert!(!std::path::Path::new(&path).exists(), "音声が消えていない");
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn marks_anki_note() {
        let (store, _dir) = store();
        let saved = store.save(sample("dog")).unwrap();
        store.mark_anki(saved.id, 1234567890).unwrap();
        assert_eq!(store.list(1, 0).unwrap()[0].anki_note_id, Some(1234567890));
    }
}
