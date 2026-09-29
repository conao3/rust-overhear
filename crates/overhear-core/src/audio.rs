//! PipeWire からのシステム音声キャプチャ。
//!
//! MVP は `pw-record` を subprocess として起動し、stdout の raw PCM を読む。
//! `stream.capture.sink=true` を渡すと既定シンクの monitor に接続されるため、
//! ブラウザ・プレイヤー・会議アプリのいずれが鳴らしていても拾える。

use std::io::Read;
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result};

use crate::child::die_with_parent;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct CaptureConfig {
    pub sample_rate: u32,
    /// pw-record の --target。None なら既定シンク。
    pub target: Option<String>,
    /// true でシンクの monitor を掴む (システム音声)。false ならマイク等の source。
    pub capture_sink: bool,
    pub latency: String,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16_000,
            target: None,
            capture_sink: true,
            latency: "100ms".to_string(),
        }
    }
}

pub struct CaptureHandle {
    child: Child,
}

impl CaptureHandle {
    pub fn stop(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// pw-record を起動し、PCM16 mono のチャンクを tx へ流す。
pub fn spawn(cfg: &CaptureConfig, tx: mpsc::UnboundedSender<Vec<i16>>) -> Result<CaptureHandle> {
    let mut cmd = Command::new("pw-record");
    cmd.arg("--rate")
        .arg(cfg.sample_rate.to_string())
        .arg("--channels")
        .arg("1")
        .arg("--format")
        .arg("s16")
        .arg("--latency")
        .arg(&cfg.latency);

    if cfg.capture_sink {
        cmd.arg("-P").arg("stream.capture.sink=true");
    }
    if let Some(target) = &cfg.target {
        cmd.arg("--target").arg(target);
    }
    cmd.arg("-");

    cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    // 親が落ちても pw-record が残らないようにする。
    let mut child = die_with_parent(&mut cmd)
        .spawn()
        .context("pw-record の起動に失敗した (PipeWire は動いているか)")?;

    let mut stdout = child
        .stdout
        .take()
        .context("pw-record の stdout が取れない")?;

    // 128ms 相当 (16kHz なら 2048 サンプル) ずつ読む。
    let chunk_samples = (cfg.sample_rate as usize / 1000) * 128;
    std::thread::spawn(move || {
        let mut raw = vec![0u8; chunk_samples * 2];
        loop {
            match stdout.read(&mut raw) {
                Ok(0) => break,
                Ok(n) => {
                    // 端数バイトは切り捨てる (次の read で整合する)
                    let n = n - (n % 2);
                    let samples: Vec<i16> = raw[..n]
                        .chunks_exact(2)
                        .map(|b| i16::from_le_bytes([b[0], b[1]]))
                        .collect();
                    if tx.send(samples).is_err() {
                        break; // 受け手が落ちた
                    }
                }
                Err(err) => {
                    tracing::warn!(?err, "pw-record の読み取りが失敗した");
                    break;
                }
            }
        }
        tracing::info!("音声キャプチャのストリームが終了した");
    });

    Ok(CaptureHandle { child })
}
