//! ログイン時の自動起動 (XDG autostart)。
//!
//! `$XDG_CONFIG_HOME/autostart/overhear.desktop` の有無がそのまま設定になる。
//! デスクトップ環境の「自動起動アプリ」の画面から消しても整合が崩れない。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// 自動起動で立ち上がったことを示す引数。スタジオを出さずに始める。
pub const AUTOSTART_ARGS: &str = "app start-app --autostart";

const FILE_NAME: &str = "overhear.desktop";

pub fn autostart_dir() -> Result<PathBuf> {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|config| config.join("autostart"))
        .context("自動起動の置き場所を決められない")
}

/// desktop entry の Exec に書くコマンド。
///
/// nix のパッケージでは current_exe が wrapper の中の実体を指し、そのまま書くと
/// wrapper が付ける環境変数 (モデルや辞書のパス) が抜ける。パッケージは
/// `OVERHEAR_AUTOSTART_EXEC` で起動コマンドを渡す。開発中は current_exe を使う。
pub fn exec_command() -> Result<String> {
    match std::env::var("OVERHEAR_AUTOSTART_EXEC") {
        Ok(exec) => Ok(exec),
        Err(_) => Ok(std::env::current_exe()
            .context("current_exe")?
            .display()
            .to_string()),
    }
}

pub fn is_enabled(dir: &Path) -> bool {
    dir.join(FILE_NAME).exists()
}

fn entry(exec: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=overhear\n\
         Comment=システム音声を字幕にして語学学習に使う\n\
         Exec={exec} {AUTOSTART_ARGS}\n\
         Icon=overhear\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

pub fn enable(dir: &Path, exec: &str) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("{} を作れない", dir.display()))?;
    let path = dir.join(FILE_NAME);
    std::fs::write(&path, entry(exec)).with_context(|| format!("{} に書けない", path.display()))
}

pub fn refresh(dir: &Path) -> Result<bool> {
    let path = dir.join(FILE_NAME);
    let current = match std::fs::read_to_string(&path) {
        Ok(current) => current,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err).with_context(|| format!("{} を読めない", path.display())),
    };
    let mut changed = false;
    let lines: Vec<String> = current
        .lines()
        .map(|line| {
            match line
                .strip_prefix("Exec=")
                .and_then(|exec| exec.strip_suffix(" --autostart"))
            {
                Some(exec) if !line.ends_with(AUTOSTART_ARGS) => {
                    changed = true;
                    format!("Exec={exec} {AUTOSTART_ARGS}")
                }
                _ => line.to_string(),
            }
        })
        .collect();
    if !changed {
        return Ok(false);
    }
    std::fs::write(&path, lines.join("\n") + "\n")
        .with_context(|| format!("{} に書けない", path.display()))?;
    Ok(true)
}

pub fn disable(dir: &Path) -> Result<()> {
    let path = dir.join(FILE_NAME);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("{} を消せない", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enable_then_disable() {
        let dir = std::env::temp_dir().join(format!("overhear-autostart-{}", std::process::id()));
        assert!(!is_enabled(&dir));

        enable(&dir, "overhear").unwrap();
        assert!(is_enabled(&dir));
        let entry = std::fs::read_to_string(dir.join(FILE_NAME)).unwrap();
        assert!(entry.contains("\nExec=overhear app start-app --autostart\n"));

        disable(&dir).unwrap();
        assert!(!is_enabled(&dir));
        // 無い状態で消しても失敗しない。
        disable(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn refresh_rewrites_the_flat_autostart_flag() {
        let dir =
            std::env::temp_dir().join(format!("overhear-autostart-refresh-{}", std::process::id()));
        assert!(!refresh(&dir).unwrap());

        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        std::fs::write(
            &path,
            "[Desktop Entry]\nExec=/opt/overhear --autostart\nIcon=overhear\n",
        )
        .unwrap();
        assert!(refresh(&dir).unwrap());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[Desktop Entry]\nExec=/opt/overhear app start-app --autostart\nIcon=overhear\n"
        );
        assert!(!refresh(&dir).unwrap());

        enable(&dir, "overhear").unwrap();
        assert!(!refresh(&dir).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
