//! TokenBar 自己的狀態檔（`~/.tokenbar/state.json`），記錄安裝前被取代的設定，供移除時還原。

use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use super::Result;

pub fn read(dir: &Path) -> Map<String, Value> {
    fs::read_to_string(dir.join("state.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| match v {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default()
}

pub fn write(dir: &Path, state: &Map<String, Value>) -> Result<()> {
    fs::create_dir_all(dir)?;
    let text = serde_json::to_string_pretty(state).expect("state is serializable");
    fs::write(dir.join("state.json"), format!("{text}\n"))?;
    Ok(())
}
