//! 二重起動の防止。
//!
//! `$XDG_RUNTIME_DIR/overhear.lock` を flock で掴めたほうが本体になり、
//! `overhear.sock` で待ち受ける。掴めなかった 2 つ目はソケットへ知らせて終わり、
//! 本体がスタジオを前に出す。D-Bus (zbus) で待ち受ける方式は、起動時に GTK の
//! メインスレッドを止めてウィンドウが描画されなくなるので使わない。

use std::fs::File;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use anyhow::{Context, Result};

pub enum Instance {
    /// 本体。ロックを持ち続け、ソケットで 2 つ目からの知らせを受ける。
    Primary { lock: File, listener: UnixListener },
    /// 既に本体が動いている。
    Secondary,
}

fn runtime_dir() -> Result<PathBuf> {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR が未設定")
}

pub fn acquire() -> Result<Instance> {
    let dir = runtime_dir()?;
    let lock_path = dir.join("overhear.lock");
    let socket_path = dir.join("overhear.sock");
    let lock =
        File::create(&lock_path).with_context(|| format!("{} を作れない", lock_path.display()))?;
    let locked = unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    if !locked {
        // 本体が待ち受けを始める前なら繋がらないが、そのときは知らせずに終わる。
        if let Ok(mut stream) = UnixStream::connect(&socket_path) {
            let _ = stream.write_all(b"show\n");
        }
        return Ok(Instance::Secondary);
    }
    // ロックを持っているので、残っているソケットは前の本体の残骸。
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("{} で待ち受けられない", socket_path.display()))?;
    Ok(Instance::Primary { lock, listener })
}

/// 2 つ目からの知らせを受けるたびに `on_show` を呼ぶ。
pub fn serve(listener: UnixListener, on_show: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if stream.is_ok() {
                on_show();
            }
        }
    });
}
