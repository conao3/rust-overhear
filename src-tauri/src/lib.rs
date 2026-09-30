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

mod autostart;
mod server;
mod single_instance;

use anyhow::{Context, Result};
use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tauri_plugin_window_state::{AppHandleExt, StateFlags, WindowExt};

/// 字幕バーの高さ。画面下部にこの高さで貼り付ける。
const CAPTION_HEIGHT: f64 = 150.0;
/// 字幕バーの幅を画面幅のどれだけにするか。
const CAPTION_WIDTH_RATIO: f64 = 0.72;
/// 画面下端からの余白。
const CAPTION_BOTTOM_MARGIN: f64 = 64.0;

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

    // 画面下部の中央へ寄せ、前回動かした位置と大きさがあればそちらに戻す。
    // モニタが取れない環境では既定位置のままにする。
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let size = monitor.size().to_logical::<f64>(monitor.scale_factor());
        let width = size.width * CAPTION_WIDTH_RATIO;
        let _ = window.set_size(tauri::LogicalSize::new(width, CAPTION_HEIGHT));
        let _ = window.set_position(tauri::LogicalPosition::new(
            (size.width - width) / 2.0,
            size.height - CAPTION_HEIGHT - CAPTION_BOTTOM_MARGIN,
        ));
    }
    if let Err(err) = window.restore_state(window_state_flags()) {
        tracing::warn!(%err, "字幕バーの位置を戻せなかった");
    }
    Ok(window)
}

/// 残すのは位置と大きさだけ。表示状態は起動のしかた (自動起動か) で決める。
fn window_state_flags() -> StateFlags {
    StateFlags::POSITION | StateFlags::SIZE | StateFlags::MAXIMIZED
}

/// 動かした・大きさを変えたら、落ち着いたところでディスクへ書く。
///
/// プラグインが書くのは正常終了のときだけで、ログアウトやシグナルで
/// 終わると動かした位置が残らない。
fn save_window_state_on_change(window: &WebviewWindow) {
    let generation = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let app = window.app_handle().clone();
    window.on_window_event(move |event| {
        if !matches!(
            event,
            tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)
        ) {
            return;
        }
        use std::sync::atomic::Ordering;
        let mine = generation.fetch_add(1, Ordering::Relaxed) + 1;
        let generation = std::sync::Arc::clone(&generation);
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if generation.load(Ordering::Relaxed) == mine {
                if let Err(err) = app.save_window_state(window_state_flags()) {
                    tracing::warn!(%err, "ウィンドウの位置を保存できなかった");
                }
            }
        });
    });
}

fn build_studio_window(app: &AppHandle, script: &str, visible: bool) -> Result<WebviewWindow> {
    Ok(
        WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
            .title("overhear")
            .inner_size(960.0, 720.0)
            .visible(visible)
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
    let autostart_dir = autostart::autostart_dir()?;
    let launch_at_login = CheckMenuItem::with_id(
        app,
        "launch_at_login",
        "ログイン時に起動",
        true,
        autostart::is_enabled(&autostart_dir),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&toggle_caption, &open_studio, &launch_at_login, &quit],
    )?;

    TrayIconBuilder::with_id("overhear")
        .icon(
            app.default_window_icon()
                .cloned()
                .context("トレイ用アイコン")?,
        )
        .tooltip("overhear")
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "toggle_caption" => toggle_window(app, "caption"),
            "open_studio" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "launch_at_login" => {
                // クリックでチェックは既に反転している。ファイルの実態に合わせ直す。
                let result = if autostart::is_enabled(&autostart_dir) {
                    autostart::disable(&autostart_dir)
                } else {
                    autostart::exec_command()
                        .and_then(|exec| autostart::enable(&autostart_dir, &exec))
                };
                if let Err(err) = result {
                    tracing::warn!(%err, "自動起動を切り替えられなかった");
                }
                let _ = launch_at_login.set_checked(autostart::is_enabled(&autostart_dir));
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

    // 2 つ目を起動したら、そちらは終わって既に動いているほうのスタジオを出す。
    let listener = match single_instance::acquire() {
        Ok(single_instance::Instance::Secondary) => {
            tracing::info!("overhear は既に動いているので、そちらのスタジオを出して終わる");
            return;
        }
        Ok(single_instance::Instance::Primary { lock, listener }) => {
            // ロックはプロセスが終わるまで持つ。
            std::mem::forget(lock);
            Some(listener)
        }
        Err(err) => {
            tracing::warn!(%err, "二重起動の確認ができないので、確認せずに起動する");
            None
        }
    };

    tauri::Builder::default()
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(window_state_flags())
                // 字幕バーは既定の位置へ置いてから戻すので、自動では戻さない。
                .skip_initial_state("caption")
                .build(),
        )
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
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
        .setup(move |app| {
            let from_autostart = std::env::args().any(|a| a == autostart::AUTOSTART_ARG);
            if let Some(listener) = listener {
                let show_handle = app.handle().clone();
                single_instance::serve(listener, move || {
                    if let Some(window) = show_handle.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                });
            }
            // 他のアプリが同じキーを使っていても起動は続ける。トレイからは操作できる。
            for shortcut in [caption_shortcut, studio_shortcut] {
                if let Err(err) = app.global_shortcut().register(shortcut) {
                    tracing::warn!(%err, ?shortcut, "グローバルホットキーを登録できなかった");
                }
            }
            let (supervisor, announce) = server::Supervisor::start()?;
            tracing::info!(endpoint = %announce.graphql, "overhear-server に接続する");
            let handle = app.handle();
            let script = announce.init_script();
            let caption = build_caption_window(handle, &script)?;
            // ログイン時はスタジオを出さない。字幕バーとトレイだけで待つ。
            let studio = build_studio_window(handle, &script, !from_autostart)?;
            save_window_state_on_change(&caption);
            save_window_state_on_change(&studio);
            build_tray(handle)?;

            // サーバが落ちたら起動し直し、各ウィンドウを新しい接続先で読み込み直す。
            let reload_handle = handle.clone();
            supervisor.watch(move |announce| {
                let script = announce.reconnect_script();
                for label in ["caption", "main"] {
                    if let Some(window) = reload_handle.get_webview_window(label) {
                        if let Err(err) = window.eval(&script) {
                            tracing::warn!(%err, label, "ウィンドウを繋ぎ直せなかった");
                        }
                    }
                }
            });
            // サーバの寿命をアプリに合わせる。
            app.manage(supervisor);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Tauri アプリの起動に失敗した")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                app.state::<std::sync::Arc<server::Supervisor>>().stop();
            }
        });
}
