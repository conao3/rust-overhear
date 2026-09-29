//! overhear のコア。システム音声のキャプチャ、音声認識、翻訳を受け持つ。
//!
//! GraphQL / Tauri といった外側の層はこの crate を射影するだけにしてある。

pub mod anki;
pub mod asr;
pub mod audio;
pub mod child;
pub mod devices;
pub mod dict;
pub mod gate;
pub mod model;
pub mod pipeline;
pub mod ring;
pub mod settings;
pub mod translate;
pub mod vocab;

pub use pipeline::{EngineChoice, Overhear, RuntimeConfig};

use std::path::PathBuf;

/// 語彙・音声・設定の置き場所。
///
/// `OVERHEAR_DATA_DIR` → `$XDG_DATA_HOME/overhear` → `~/.local/share/overhear` の順に決める。
pub fn data_dir() -> anyhow::Result<PathBuf> {
    std::env::var("OVERHEAR_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("XDG_DATA_HOME").map(|d| PathBuf::from(d).join("overhear")))
        .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".local/share/overhear")))
        .map_err(|_| anyhow::anyhow!("データディレクトリを決められない"))
}
