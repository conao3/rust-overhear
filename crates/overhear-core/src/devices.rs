//! PipeWire のノード一覧。どこの音を拾うかを選ばせるために使う。
//!
//! `pw-dump` の JSON を読む。`wpctl` の整形出力より解析が安定する。

use std::process::Command;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceKind {
    /// 再生側。monitor を掴めばシステム音声になる。
    Sink,
    /// 録音側。マイクなど。
    Source,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevice {
    /// pw-record の --target に渡す値 (ノード名)。
    pub id: String,
    pub name: String,
    pub description: String,
    pub kind: DeviceKind,
    pub is_default: bool,
}

#[derive(Deserialize)]
struct PwNode {
    #[serde(rename = "type")]
    node_type: String,
    info: Option<PwInfo>,
}

#[derive(Deserialize)]
struct PwInfo {
    props: Option<serde_json::Value>,
}

/// 既定シンク / 既定ソースの名前を取る。
fn defaults() -> (Option<String>, Option<String>) {
    let Ok(output) = Command::new("pw-metadata")
        .arg("-n")
        .arg("default")
        .output()
    else {
        return (None, None);
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut sink = None;
    let mut source = None;
    for line in text.lines() {
        // 例: ... key:'default.audio.sink' value:'{"name":"alsa_output..."}' ...
        let Some((key, rest)) = line.split_once("key:'") else {
            continue;
        };
        let _ = key;
        let Some((key, rest)) = rest.split_once('\'') else {
            continue;
        };
        let Some(value) = rest
            .split_once("value:'")
            .and_then(|(_, v)| v.split_once('\''))
            .map(|(v, _)| v)
        else {
            continue;
        };
        let name = serde_json::from_str::<serde_json::Value>(value)
            .ok()
            .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string));
        match key {
            "default.audio.sink" => sink = name,
            "default.audio.source" => source = name,
            _ => {}
        }
    }
    (sink, source)
}

/// 音声の入出力ノードを列挙する。
pub fn list() -> Result<Vec<AudioDevice>> {
    let output = Command::new("pw-dump")
        .output()
        .context("pw-dump を実行できない (PipeWire は動いているか)")?;
    let nodes: Vec<PwNode> =
        serde_json::from_slice(&output.stdout).context("pw-dump の出力を解釈できない")?;

    let (default_sink, default_source) = defaults();

    let mut devices = Vec::new();
    for node in nodes {
        if node.node_type != "PipeWire:Interface:Node" {
            continue;
        }
        let Some(props) = node.info.and_then(|i| i.props) else {
            continue;
        };
        let get = |key: &str| {
            props
                .get(key)
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_default()
        };
        let kind = match get("media.class").as_str() {
            "Audio/Sink" => DeviceKind::Sink,
            "Audio/Source" => DeviceKind::Source,
            _ => continue,
        };
        let name = get("node.name");
        if name.is_empty() {
            continue;
        }
        let is_default = match kind {
            DeviceKind::Sink => default_sink.as_deref() == Some(name.as_str()),
            DeviceKind::Source => default_source.as_deref() == Some(name.as_str()),
        };
        let description = {
            let d = get("node.description");
            if d.is_empty() { name.clone() } else { d }
        };
        devices.push(AudioDevice {
            id: name.clone(),
            name,
            description,
            kind,
            is_default,
        });
    }

    // 再生側を先に、既定を各グループの先頭に。
    devices.sort_by_key(|d| {
        (
            matches!(d.kind, DeviceKind::Source),
            !d.is_default,
            d.description.clone(),
        )
    });
    Ok(devices)
}
