//! two-pass ASR の後段。april の即時出力を whisper.cpp の確定文へ差し替える。
//!
//! `whisper-server` を子プロセスとして 1 回だけ起動し、確定した segment の
//! 音声を `/inference` へ投げる。モデルの読み込みが 1 度で済むので、
//! segment ごとに `whisper-cli` を起動するより桁違いに速い。
//!
//! `pw-record` と同じく、まずは subprocess で始めて実用を優先している。
//! in-process 化 (whisper-rs) はいつでもここの内側に閉じて差し替えられる。

use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use crate::ring::encode_wav;

/// 発話の直前を少し含めて切り出す。先頭が欠けた音声は認識が落ちる。
const LEAD_IN_MS: u64 = 200;

#[derive(Deserialize)]
struct InferenceResponse {
    text: String,
}

pub struct WhisperRefiner {
    endpoint: String,
    client: reqwest::Client,
    /// Drop でサーバを道連れにする。
    child: Child,
}

impl Drop for WhisperRefiner {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 空いているポートを 1 つ選ぶ。whisper-server は 0 番を解釈しないため、
/// 一度 bind して番号だけ取り、すぐ手放す。
fn pick_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").context("空きポートの確保")?;
    Ok(listener.local_addr()?.port())
}

impl WhisperRefiner {
    /// `WHISPER_MODEL_PATH` のモデルで whisper-server を起動する。
    pub fn spawn_from_env(threads: usize) -> Result<Self> {
        let model = std::env::var("WHISPER_MODEL_PATH")
            .context("WHISPER_MODEL_PATH が未設定 (nix develop の外で実行していないか)")?;
        Self::spawn(&model, threads)
    }

    pub fn spawn(model_path: &str, threads: usize) -> Result<Self> {
        if !std::path::Path::new(model_path).exists() {
            return Err(anyhow!("whisper のモデルが見つからない: {model_path}"));
        }
        let port = pick_port()?;
        let child = Command::new("whisper-server")
            .arg("-m")
            .arg(model_path)
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("-t")
            .arg(threads.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("whisper-server の起動に失敗した (PATH にあるか)")?;

        tracing::info!(port, model = %model_path, "whisper-server を起動した");
        Ok(Self {
            endpoint: format!("http://127.0.0.1:{port}"),
            client: reqwest::Client::new(),
            child,
        })
    }

    /// モデルの読み込みが終わって応答するまで待つ。
    pub async fn wait_ready(&self, timeout: Duration) -> Result<()> {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if self.client.get(&self.endpoint).send().await.is_ok() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        Err(anyhow!("whisper-server が起動しなかった"))
    }

    /// PCM16 mono を投げて確定文を受け取る。
    pub async fn refine(&self, pcm: &[i16], sample_rate: u32) -> Result<String> {
        if pcm.is_empty() {
            return Err(anyhow!("音声が空"));
        }
        let wav = encode_wav(pcm, sample_rate);
        let part = reqwest::multipart::Part::bytes(wav)
            .file_name("segment.wav")
            .mime_str("audio/wav")?;
        let form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("response_format", "json");

        let resp = self
            .client
            .post(format!("{}/inference", self.endpoint))
            .multipart(form)
            .send()
            .await
            .context("whisper-server に送れない")?;
        if !resp.status().is_success() {
            return Err(anyhow!("whisper-server が {} を返した", resp.status()));
        }
        let parsed: InferenceResponse = resp
            .json()
            .await
            .context("whisper-server の応答を解釈できない")?;
        Ok(strip_non_speech(&normalize(&parsed.text)))
    }

    /// 切り出しの開始位置。発話の直前を少し含める。
    pub fn lead_in(start_ms: u64) -> u64 {
        start_ms.saturating_sub(LEAD_IN_MS)
    }
}

/// whisper は行頭の空白と改行を挟んで返すので 1 行に畳む。
fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 非発話マーカー (`[MUSIC PLAYING]` / `(upbeat music)`) を落とす。
///
/// 発話に混ざっている場合だけ取り除き、全体がマーカーだけの区間は
/// そのまま残す。台詞が無いことを字幕で示すほうが分かりやすいため。
fn strip_non_speech(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth_square = 0usize;
    let mut depth_paren = 0usize;
    for ch in text.chars() {
        match ch {
            '[' => depth_square += 1,
            ']' => depth_square = depth_square.saturating_sub(1),
            '(' => depth_paren += 1,
            ')' => depth_paren = depth_paren.saturating_sub(1),
            _ if depth_square == 0 && depth_paren == 0 => out.push(ch),
            _ => {}
        }
    }
    let stripped = normalize(&out);
    if stripped.is_empty() {
        text.to_string()
    } else {
        stripped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_multiline_output() {
        let raw = " If I have to spend another second waiting around doing\n nothing, I'm gonna lose my mind.\n";
        assert_eq!(
            normalize(raw),
            "If I have to spend another second waiting around doing nothing, I'm gonna lose my mind."
        );
    }

    #[test]
    fn strips_markers_only_when_speech_remains() {
        // 発話に混ざったマーカーは落とす
        assert_eq!(
            strip_non_speech("[MUSIC PLAYING] I have to go now."),
            "I have to go now."
        );
        assert_eq!(
            strip_non_speech("Well (laughs) that is fine."),
            "Well that is fine."
        );
        // 全体がマーカーなら残す (台詞が無いことを示すため)
        assert_eq!(strip_non_speech("(upbeat music)"), "(upbeat music)");
        assert_eq!(strip_non_speech("[MUSIC PLAYING]"), "[MUSIC PLAYING]");
    }

    #[test]
    fn lead_in_does_not_underflow() {
        assert_eq!(WhisperRefiner::lead_in(50), 0);
        assert_eq!(WhisperRefiner::lead_in(1000), 800);
    }
}
