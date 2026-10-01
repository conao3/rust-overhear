//! PipeWire からのシステム音声キャプチャ。
//!
//! MVP は `pw-record` を subprocess として起動し、stdout の raw PCM を読む。
//! `stream.capture.sink=true` を渡すと既定シンクの monitor に接続されるため、
//! ブラウザ・プレイヤー・会議アプリのいずれが鳴らしていても拾える。

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
            latency: "10ms".to_string(),
        }
    }
}

pub struct CaptureHandle {
    child: Child,
    /// 読み取りスレッドが動いているか。pw-record が終了すると false になる。
    alive: Arc<AtomicBool>,
}

impl CaptureHandle {
    /// まだ PCM を受け取っているか。既定のデバイスが抜かれたり PipeWire が
    /// 再起動したりすると pw-record は終了する。
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

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
        .arg("--container")
        .arg("raw")
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

    // 128ms 相当まで一度に読む。read はバッファが埋まるのを待たず、届いた分を返す。
    let chunk_samples = (cfg.sample_rate as usize / 1000) * 128;
    let alive = Arc::new(AtomicBool::new(true));
    let reader_alive = Arc::clone(&alive);
    std::thread::spawn(move || {
        let mut raw = vec![0u8; chunk_samples * 2];
        let mut pending = None;
        loop {
            match stdout.read(&mut raw) {
                Ok(0) => break,
                Ok(n) => {
                    let samples = decode_pcm(&raw[..n], &mut pending);
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
        reader_alive.store(false, Ordering::Relaxed);
        tracing::info!("音声キャプチャのストリームが終了した");
    });

    Ok(CaptureHandle { child, alive })
}

fn decode_pcm(bytes: &[u8], pending: &mut Option<u8>) -> Vec<i16> {
    let mut samples = Vec::with_capacity((bytes.len() + usize::from(pending.is_some())) / 2);
    let mut bytes = bytes.iter().copied();
    while let Some(byte) = bytes.next() {
        if let Some(first) = pending.take() {
            samples.push(i16::from_le_bytes([first, byte]));
        } else if let Some(second) = bytes.next() {
            samples.push(i16::from_le_bytes([byte, second]));
        } else {
            *pending = Some(byte);
        }
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_survives_reads_split_inside_a_sample() {
        let expected = [-32768i16, -1, 0, 1, 32767];
        let bytes: Vec<_> = expected.iter().flat_map(|s| s.to_le_bytes()).collect();
        for size in 1..=bytes.len() {
            let mut pending = None;
            let actual: Vec<_> = bytes
                .chunks(size)
                .flat_map(|part| decode_pcm(part, &mut pending))
                .collect();
            assert_eq!(actual, expected);
            assert_eq!(pending, None);
        }
    }
}
