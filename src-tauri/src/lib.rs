//! Tauri 側の責務はウィンドウとプロセス管理だけに絞ってある。
//!
//! ドメインロジックは overhear-server (GraphQL) が持ち、WebView は
//! Apollo Client で 127.0.0.1 の HTTP / WebSocket に話しかける。
//! 起動時に生成されたポートとトークンを初期化スクリプトで流し込む。

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

#[derive(Debug, Deserialize)]
struct Announce {
    graphql: String,
    websocket: String,
    token: Option<String>,
}

/// 終了時に子プロセスを道連れにする。
struct ServerProcess(Child);

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
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
fn start_server() -> Result<(Announce, ServerProcess)> {
    let bin = server_binary()?;
    let mut child = Command::new(&bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("{} の起動", bin.display()))?;

    let stdout = child.stdout.take().context("サーバの stdout が取れない")?;
    let reader = BufReader::new(stdout);

    // サーバはログを stderr に出すので stdout の 1 行目が接続情報になるが、
    // 何かが紛れ込んでも拾えるよう、JSON として読める行を先頭から探す。
    for line in reader.lines().take(20) {
        let line = line.context("サーバの接続情報を読めない")?;
        if let Ok(announce) = serde_json::from_str::<Announce>(line.trim()) {
            return Ok((announce, ServerProcess(child)));
        }
    }
    Err(anyhow!("サーバが接続情報を出力しなかった"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,overhear=debug".into()),
        )
        .init();

    tauri::Builder::default()
        .setup(|app| {
            let (announce, process) = start_server()?;
            tracing::info!(endpoint = %announce.graphql, "overhear-server に接続する");

            // フロントは window.__OVERHEAR__ から接続先とトークンを読む。
            let payload = serde_json::json!({
                "graphql": announce.graphql,
                "websocket": announce.websocket,
                "token": announce.token,
            });
            let script = format!("window.__OVERHEAR__ = {payload};");

            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("overhear")
                .inner_size(960.0, 720.0)
                .initialization_script(&script)
                .build()?;

            // サーバの寿命をアプリに合わせる。
            app.manage(process);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri アプリの起動に失敗した");
}
