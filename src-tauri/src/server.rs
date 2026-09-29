//! overhear-server の起動と監視。
//!
//! サーバが落ちたら起動し直す。ポートとトークンは起動ごとに変わるので、
//! 新しい接続情報をウィンドウへ渡してページを読み込み直す。

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use overhear_core::child::die_with_parent;
use serde::Deserialize;

/// この時間の中でこれ以上落ちたら、起動し直すのをやめる。
const RESTART_WINDOW: Duration = Duration::from_secs(60);
const MAX_RESTARTS_IN_WINDOW: usize = 5;

/// ページを読み込み直しても接続情報が残るよう、sessionStorage に置くキー。
const ENDPOINT_KEY: &str = "overhear-endpoint";

#[derive(Debug, Clone, Deserialize)]
pub struct Announce {
    pub graphql: String,
    pub websocket: String,
    pub token: Option<String>,
}

impl Announce {
    fn payload(&self) -> serde_json::Value {
        serde_json::json!({
            "graphql": self.graphql,
            "websocket": self.websocket,
            "token": self.token,
        })
    }

    /// フロントは `window.__OVERHEAR__` から接続先とトークンを読む。
    ///
    /// サーバを起動し直したあとは sessionStorage に新しい値が入っているので、
    /// そちらを優先する。
    pub fn init_script(&self) -> String {
        format!(
            "window.__OVERHEAR__ = JSON.parse(sessionStorage.getItem({key}) ?? 'null') ?? {payload};",
            key = serde_json::Value::from(ENDPOINT_KEY),
            payload = self.payload()
        )
    }

    /// 起動し直したサーバへ繋ぎ替えるためにウィンドウで実行する JS。
    pub fn reconnect_script(&self) -> String {
        format!(
            "sessionStorage.setItem({key}, {value}); location.reload();",
            key = serde_json::Value::from(ENDPOINT_KEY),
            value = serde_json::Value::from(self.payload().to_string())
        )
    }
}

/// overhear-server の実体を探す。開発中は同じ target ディレクトリに並ぶ。
fn server_binary() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("current_exe")?;
    let sibling = exe
        .parent()
        .map(|dir| dir.join("overhear-server"))
        .filter(|p| p.exists());
    sibling.ok_or_else(|| anyhow!("overhear-server が見つからない (先に cargo build すること)"))
}

/// サーバを起動し、標準出力の 1 行目に出る接続情報を受け取る。
fn spawn() -> Result<(Announce, Child)> {
    let bin = server_binary()?;
    let mut command = Command::new(&bin);
    command.stdout(Stdio::piped()).stderr(Stdio::inherit());
    // アプリが SIGKILL されても、april を抱えたサーバが残らないようにする。
    let mut child = die_with_parent(&mut command)
        .spawn()
        .with_context(|| format!("{} の起動", bin.display()))?;

    let stdout = child.stdout.take().context("サーバの stdout が取れない")?;
    let reader = BufReader::new(stdout);

    // サーバはログを stderr に出すので stdout の 1 行目が接続情報になるが、
    // 何かが紛れ込んでも拾えるよう、JSON として読める行を先頭から探す。
    for line in reader.lines().take(20) {
        let line = line.context("サーバの接続情報を読めない")?;
        if let Ok(announce) = serde_json::from_str::<Announce>(line.trim()) {
            return Ok((announce, child));
        }
    }
    let _ = child.kill();
    Err(anyhow!("サーバが接続情報を出力しなかった"))
}

/// サーバの子プロセスを持ち、落ちたら起動し直す。
pub struct Supervisor {
    child: Mutex<Option<Child>>,
    stopping: AtomicBool,
}

impl Supervisor {
    pub fn start() -> Result<(Arc<Self>, Announce)> {
        let (announce, child) = spawn()?;
        let supervisor = Arc::new(Self {
            child: Mutex::new(Some(child)),
            stopping: AtomicBool::new(false),
        });
        Ok((supervisor, announce))
    }

    /// 監視スレッドを立てる。起動し直すたびに `on_restart` へ新しい接続情報を渡す。
    pub fn watch(self: &Arc<Self>, on_restart: impl Fn(&Announce) + Send + 'static) {
        let this = Arc::clone(self);
        std::thread::spawn(move || {
            let mut restarts: VecDeque<Instant> = VecDeque::new();
            loop {
                std::thread::sleep(Duration::from_secs(1));
                if this.stopping.load(Ordering::Relaxed) {
                    return;
                }
                let status = {
                    let Ok(mut slot) = this.child.lock() else {
                        return;
                    };
                    match slot.as_mut().map(|c| c.try_wait()) {
                        Some(Ok(Some(status))) => status,
                        _ => continue,
                    }
                };
                tracing::warn!(%status, "overhear-server が終了した");

                let now = Instant::now();
                restarts.retain(|t| now.duration_since(*t) < RESTART_WINDOW);
                if restarts.len() >= MAX_RESTARTS_IN_WINDOW {
                    tracing::error!(
                        "overhear-server が短い間に落ち続けるので、起動し直すのをやめる"
                    );
                    return;
                }
                restarts.push_back(now);

                match spawn() {
                    Ok((announce, child)) => {
                        if let Ok(mut slot) = this.child.lock() {
                            *slot = Some(child);
                        }
                        tracing::info!(endpoint = %announce.graphql, "overhear-server を起動し直した");
                        on_restart(&announce);
                    }
                    Err(err) => tracing::warn!(%err, "overhear-server を起動し直せなかった"),
                }
            }
        });
    }

    /// サーバを止め、以後は起動し直さない。アプリの終了時に呼ぶ。
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::Relaxed);
        if let Ok(mut slot) = self.child.lock() {
            if let Some(mut child) = slot.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_carry_the_endpoint() {
        let announce = Announce {
            graphql: "http://127.0.0.1:1/graphql".into(),
            websocket: "ws://127.0.0.1:1/graphql".into(),
            token: Some("t".into()),
        };
        let init = announce.init_script();
        assert!(init.starts_with(
            "window.__OVERHEAR__ = JSON.parse(sessionStorage.getItem(\"overhear-endpoint\")"
        ));
        assert!(init.contains("\"token\":\"t\""));
        // sessionStorage には JSON を文字列として入れる。
        let reconnect = announce.reconnect_script();
        assert!(reconnect.contains(r#"sessionStorage.setItem("overhear-endpoint", "{\"graphql\""#));
        assert!(reconnect.ends_with("location.reload();"));
    }
}
