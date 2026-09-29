//! Tauri 側の責務はウィンドウとプロセス管理だけに絞ってある。
//!
//! ドメインロジックは overhear-server (GraphQL) が持ち、WebView は
//! Apollo Client で 127.0.0.1 の HTTP / WebSocket に話しかける。
//! 起動時に生成されたポートとトークンを初期化スクリプトで流し込む。
//!
//! ウィンドウは 2 枚。
//!
//! - `caption` — 常時最前面・枠なしの字幕バー。動画の上に重ねて使う
//! - `main`   — 履歴・語彙・設定を操作するスタジオ

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result, anyhow};
use overhear_core::child::die_with_parent;
use serde::Deserialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};

/// 字幕バーの高さ。画面下部にこの高さで貼り付ける。
const CAPTION_HEIGHT: f64 = 150.0;
/// 字幕バーの幅を画面幅のどれだけにするか。
const CAPTION_WIDTH_RATIO: f64 = 0.72;
/// 画面下端からの余白。
const CAPTION_BOTTOM_MARGIN: f64 = 64.0;

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
            return Ok((announce, ServerProcess(child)));
        }
    }
    Err(anyhow!("サーバが接続情報を出力しなかった"))
}

/// フロントは `window.__OVERHEAR__` から接続先とトークンを読む。
fn init_script(announce: &Announce) -> String {
    let payload = serde_json::json!({
        "graphql": announce.graphql,
        "websocket": announce.websocket,
        "token": announce.token,
    });
    format!("window.__OVERHEAR__ = {payload};")
}

/// 常時最前面・枠なしの字幕バーを画面下部に貼る。
fn build_caption_window(app: &AppHandle, script: &str) -> Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, "caption", WebviewUrl::App("caption.html".into()))
        .title("overhear — 字幕")
        .inner_size(960.0, CAPTION_HEIGHT)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(true)
        .transparent(true)
        .initialization_script(script)
        .build()?;

    // 画面下部の中央へ寄せる。モニタが取れない環境では既定位置のままにする。
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let size = monitor.size().to_logical::<f64>(monitor.scale_factor());
        let width = size.width * CAPTION_WIDTH_RATIO;
        let _ = window.set_size(tauri::LogicalSize::new(width, CAPTION_HEIGHT));
        let _ = window.set_position(tauri::LogicalPosition::new(
            (size.width - width) / 2.0,
            size.height - CAPTION_HEIGHT - CAPTION_BOTTOM_MARGIN,
        ));
    }
    Ok(window)
}

fn build_studio_window(app: &AppHandle, script: &str) -> Result<WebviewWindow> {
    Ok(
        WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
            .title("overhear")
            .inner_size(960.0, 720.0)
            .initialization_script(script)
            .build()?,
    )
}

/// 表示中なら隠し、隠れていれば出す。
fn toggle_window(app: &AppHandle, label: &str) {
    let Some(window) = app.get_webview_window(label) else {
        return;
    };
    match window.is_visible() {
        Ok(true) => {
            let _ = window.hide();
        }
        _ => {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

fn build_tray(app: &AppHandle) -> Result<()> {
    let toggle_caption = MenuItem::with_id(
        app,
        "toggle_caption",
        "字幕バーの表示切替",
        true,
        None::<&str>,
    )?;
    let open_studio = MenuItem::with_id(app, "open_studio", "スタジオを開く", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle_caption, &open_studio, &quit])?;

    TrayIconBuilder::with_id("overhear")
        .icon(
            app.default_window_icon()
                .cloned()
                .context("トレイ用アイコン")?,
        )
        .tooltip("overhear")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle_caption" => toggle_window(app, "caption"),
            "open_studio" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,overhear=debug".into()),
        )
        .init();

    // Ctrl+Alt+O で字幕バー、Ctrl+Alt+S でスタジオを出し入れする。
    let caption_shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyO);
    let studio_shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyS);

    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcuts([caption_shortcut, studio_shortcut])
                .expect("グローバルショートカットの登録")
                .with_handler(move |app, shortcut, event| {
                    // 押し下げだけを拾う (離した分で二重に反応させない)。
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    if shortcut == &caption_shortcut {
                        toggle_window(app, "caption");
                    } else if shortcut == &studio_shortcut {
                        toggle_window(app, "main");
                    }
                })
                .build(),
        )
        .setup(|app| {
            let (announce, process) = start_server()?;
            tracing::info!(endpoint = %announce.graphql, "overhear-server に接続する");
            let handle = app.handle();
            let script = init_script(&announce);
            build_caption_window(handle, &script)?;
            build_studio_window(handle, &script)?;
            build_tray(handle)?;

            // サーバの寿命をアプリに合わせる。
            app.manage(process);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri アプリの起動に失敗した");
}
