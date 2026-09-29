//! AnkiConnect 経由のカード書き出し。
//!
//! 「文 + その区間の音声 + 単語 + 語義 + 訳」を 1 枚のノートにする。
//! 音声はローカルパスで渡すので、AnkiConnect が media に取り込む。

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:8765";
const API_VERSION: u32 = 6;

pub const DEFAULT_DECK: &str = "overhear";
pub const DEFAULT_MODEL: &str = "Basic";

#[derive(Debug, Clone)]
pub struct AnkiNote {
    pub deck: String,
    pub model: String,
    pub front: String,
    pub back: String,
    /// 添付する音声の絶対パス。
    pub audio_path: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Deserialize)]
struct AnkiResponse {
    result: Option<Value>,
    error: Option<String>,
}

pub struct AnkiConnect {
    endpoint: String,
    client: reqwest::Client,
}

impl AnkiConnect {
    pub fn from_env() -> Self {
        Self {
            endpoint: std::env::var("OVERHEAR_ANKI_ENDPOINT")
                .unwrap_or_else(|_| DEFAULT_ENDPOINT.to_string()),
            client: reqwest::Client::new(),
        }
    }

    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            client: reqwest::Client::new(),
        }
    }

    async fn call(&self, action: &str, params: Value) -> Result<Value> {
        let body = json!({ "action": action, "version": API_VERSION, "params": params });
        let resp = self
            .client
            .post(&self.endpoint)
            .json(&body)
            .send()
            .await
            .with_context(|| {
                format!(
                    "Anki に接続できない ({})。Anki を起動し AnkiConnect アドオンを入れること",
                    self.endpoint
                )
            })?;
        let parsed: AnkiResponse = resp
            .json()
            .await
            .context("AnkiConnect の応答を解釈できない")?;
        if let Some(err) = parsed.error {
            return Err(anyhow!("AnkiConnect がエラーを返した: {err}"));
        }
        Ok(parsed.result.unwrap_or(Value::Null))
    }

    /// 接続できるか。UI に理由を出すために使う。
    pub async fn version(&self) -> Result<u32> {
        let v = self.call("version", json!({})).await?;
        v.as_u64()
            .map(|n| n as u32)
            .ok_or_else(|| anyhow!("version の応答が数値でない"))
    }

    /// 無ければ作る。既にあっても成功する。
    pub async fn ensure_deck(&self, deck: &str) -> Result<()> {
        self.call("createDeck", json!({ "deck": deck })).await?;
        Ok(())
    }

    pub async fn add_note(&self, note: &AnkiNote) -> Result<i64> {
        let mut payload = json!({
            "deckName": note.deck,
            "modelName": note.model,
            "fields": { "Front": note.front, "Back": note.back },
            "options": { "allowDuplicate": false },
            "tags": note.tags,
        });

        if let Some(path) = &note.audio_path {
            let filename = std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "overhear.wav".to_string());
            payload["audio"] = json!([{
                "path": path,
                "filename": format!("overhear-{filename}"),
                "fields": ["Back"],
            }]);
        }

        let result = self.call("addNote", json!({ "note": payload })).await?;
        result
            .as_i64()
            .ok_or_else(|| anyhow!("addNote が note id を返さなかった"))
    }
}

impl Default for AnkiConnect {
    fn default() -> Self {
        Self::from_env()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    /// AnkiConnect の代わりに 1 リクエストだけ受ける最小の HTTP サーバ。
    /// 受け取った JSON を返し、固定のレスポンスを返す。
    fn mock_anki(response: &'static str) -> (String, std::thread::JoinHandle<Value>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
                if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; len];
            reader.read_exact(&mut body).unwrap();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                response.len(),
                response
            );
            stream.write_all(resp.as_bytes()).unwrap();
            stream.flush().unwrap();
            serde_json::from_slice(&body).unwrap()
        });
        (format!("http://{addr}"), handle)
    }

    #[tokio::test]
    async fn add_note_sends_expected_payload() {
        let (endpoint, handle) = mock_anki(r#"{"result":1612345678901,"error":null}"#);
        let anki = AnkiConnect::new(endpoint);
        let note = AnkiNote {
            deck: "overhear".into(),
            model: "Basic".into(),
            front: "overhear".into(),
            back: "to hear without the speaker's knowledge".into(),
            audio_path: Some("/tmp/clip.wav".into()),
            tags: vec!["overhear".into()],
        };

        let note_id = anki.add_note(&note).await.unwrap();
        assert_eq!(note_id, 1_612_345_678_901);

        let sent = handle.join().unwrap();
        assert_eq!(sent["action"], "addNote");
        assert_eq!(sent["version"], 6);
        let n = &sent["params"]["note"];
        assert_eq!(n["deckName"], "overhear");
        assert_eq!(n["fields"]["Front"], "overhear");
        // 音声は Back に添付される
        assert_eq!(n["audio"][0]["path"], "/tmp/clip.wav");
        assert_eq!(n["audio"][0]["fields"][0], "Back");
    }

    #[tokio::test]
    async fn surfaces_anki_error() {
        let (endpoint, _handle) =
            mock_anki(r#"{"result":null,"error":"cannot create note because it is a duplicate"}"#);
        let anki = AnkiConnect::new(endpoint);
        let note = AnkiNote {
            deck: "overhear".into(),
            model: "Basic".into(),
            front: "dog".into(),
            back: "a dog".into(),
            audio_path: None,
            tags: vec![],
        };
        let err = anki.add_note(&note).await.unwrap_err().to_string();
        assert!(err.contains("duplicate"), "{err}");
    }

    #[tokio::test]
    async fn reports_connection_failure_with_hint() {
        // 誰も listen していないポート
        let anki = AnkiConnect::new("http://127.0.0.1:1");
        let err = anki.version().await.unwrap_err().to_string();
        assert!(err.contains("Anki に接続できない"), "{err}");
    }
}
