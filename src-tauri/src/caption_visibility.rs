use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;

pub struct CaptionVisibility {
    path: PathBuf,
    visible: AtomicBool,
}

impl CaptionVisibility {
    pub fn load(config_dir: PathBuf) -> Self {
        let path = config_dir.join("caption-visible.json");
        let visible = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
                tracing::warn!(%err, "字幕バーの表示設定を読めなかった");
                true
            }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
            Err(err) => {
                tracing::warn!(%err, "字幕バーの表示設定を読めなかった");
                true
            }
        };
        Self {
            path,
            visible: AtomicBool::new(visible),
        }
    }

    pub fn visible(&self) -> bool {
        self.visible.load(Ordering::Relaxed)
    }

    pub fn save(&self, visible: bool) -> Result<()> {
        self.visible.store(visible, Ordering::Relaxed);
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(&visible)?)?;
        std::fs::rename(tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visibility_survives_restart_and_defaults_to_shown() {
        let dir = std::env::temp_dir().join(format!("overhear-caption-{}", std::process::id()));
        let state = CaptionVisibility::load(dir.clone());
        assert!(state.visible());
        state.save(false).unwrap();
        assert!(!state.visible());
        let restored = CaptionVisibility::load(dir.clone());
        assert!(!restored.visible());
        restored.save(true).unwrap();
        assert!(CaptionVisibility::load(dir.clone()).visible());
        std::fs::write(dir.join("caption-visible.json"), "invalid").unwrap();
        assert!(CaptionVisibility::load(dir.clone()).visible());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
