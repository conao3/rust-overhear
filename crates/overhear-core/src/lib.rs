//! overhear のコア。システム音声のキャプチャ、音声認識、翻訳を受け持つ。
//!
//! GraphQL / Tauri といった外側の層はこの crate を射影するだけにしてある。

pub mod asr;
pub mod audio;
pub mod model;
pub mod pipeline;
pub mod ring;
pub mod translate;

pub use pipeline::{EngineChoice, Overhear, RuntimeConfig};
