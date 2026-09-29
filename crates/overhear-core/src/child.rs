//! 子プロセスを親より長生きさせないための共通処理。
//!
//! 親が SIGKILL された場合 Drop は走らないため、`Command` の Drop や
//! 明示的な kill だけでは子が孤児として残る。実際に `pw-record` と
//! april-asr を抱えたサーバが複数残り、1 コアずつ食い続ける事故になった。
//! カーネル側の仕組み (PR_SET_PDEATHSIG) で確実に落とす。

use std::io;
use std::os::unix::process::CommandExt;
use std::process::Command;

/// 親プロセスが死んだら SIGKILL を受け取るよう設定する。
///
/// `pre_exec` は fork 後 exec 前に走るので、ここでの操作は
/// async-signal-safe なものに限る (prctl は該当する)。
pub fn die_with_parent(command: &mut Command) -> &mut Command {
    unsafe {
        command.pre_exec(|| {
            // PR_SET_PDEATHSIG = 1
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(io::Error::last_os_error());
            }
            // 設定の直前に親が死んでいた場合、シグナルは届かない。
            // 親が変わっていたら自分で降りる。
            if libc::getppid() == 1 {
                libc::_exit(0);
            }
            Ok(())
        });
    }
    command
}
