//! Codex CLI 的狀態列不能執行外部指令，只能從內建項目挑選（config.toml 的 `[tui] status_line`）。
//! 這裡確保內建狀態列帶有 context 與 5 小時／每週額度三個項目，並保留使用者原本的其他項目與排版。

use std::fs;
use std::path::Path;

use serde_json::json;
use toml_edit::{Array, DocumentMut, Item, Table, value};

use super::json_settings::{backup, write_atomic};
use super::{Error, Installed, Removal, Result, state};

pub const REQUIRED_ITEMS: [&str; 3] = ["context-used", "five-hour-limit", "weekly-limit"];
pub const DEFAULT_ITEMS: [&str; 6] =
    ["model-with-reasoning", "current-dir", "git-branch", "context-used", "five-hour-limit", "weekly-limit"];

/// 安裝前的狀態，移除時用來還原。
#[derive(Debug, Clone, PartialEq)]
pub struct Previous {
    /// 原本的 status_line 值（TOML 文字）；None 代表原本沒有這個鍵
    pub status_line: Option<String>,
    /// [tui] 區段是不是 TokenBar 建立的
    pub created_tui: bool,
}

fn parse(toml: &str) -> Result<DocumentMut> {
    toml.parse::<DocumentMut>().map_err(|e| Error(format!("無法解析 Codex 的 config.toml：{e}")))
}

/// 回傳修改後的 config.toml 內容與安裝前的狀態。
pub fn apply_preset(toml: &str) -> Result<(String, Previous)> {
    let mut doc = parse(toml)?;
    let created_tui = !doc.contains_key("tui");
    let tui = doc.entry("tui").or_insert_with(|| Item::Table(Table::new()));
    let tui = tui.as_table_like_mut().ok_or_else(|| Error("config.toml 的 tui 不是表格，無法設定狀態列。".into()))?;

    let previous = match tui.get_mut("status_line") {
        Some(item) => {
            // 只記值本身，不含前後空白與行尾註解；註解留在檔案裡，還原時沿用
            let original = item.as_value().map(|v| {
                let mut v = v.clone();
                v.decor_mut().clear();
                v.to_string()
            });
            let array = item
                .as_array_mut()
                .ok_or_else(|| Error("config.toml 的 tui.status_line 不是陣列，無法設定狀態列。".into()))?;
            let existing: Vec<String> = array.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect();
            for item in REQUIRED_ITEMS {
                if !existing.iter().any(|e| e == item) {
                    array.push(item);
                }
            }
            original
        }
        None => {
            tui.insert("status_line", value(DEFAULT_ITEMS.into_iter().collect::<Array>()));
            None
        }
    };
    Ok((doc.to_string(), Previous { status_line: previous, created_tui }))
}

pub fn revert_preset(toml: &str, previous: &Previous) -> Result<String> {
    let mut doc = parse(toml)?;
    let Some(tui) = doc.get_mut("tui").and_then(Item::as_table_like_mut) else { return Ok(toml.to_owned()) };
    match &previous.status_line {
        Some(original) => {
            let mut restored = original
                .trim()
                .parse::<toml_edit::Value>()
                .map_err(|e| Error(format!("無法還原原本的 status_line：{e}")))?;
            // 沿用目前這一行的空白與行尾註解
            if let Some(current) = tui.get("status_line").and_then(Item::as_value) {
                *restored.decor_mut() = current.decor().clone();
            }
            tui.insert("status_line", value(restored));
        }
        None => {
            tui.remove("status_line");
        }
    }
    if previous.created_tui && doc.get("tui").and_then(Item::as_table_like).is_some_and(|t| t.is_empty()) {
        doc.remove("tui");
    }
    Ok(doc.to_string())
}

fn read_config(file: &Path) -> Result<String> {
    match fs::read_to_string(file) {
        Ok(text) => Ok(text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(err.into()),
    }
}

pub fn install(codex_home: &Path, state_dir: &Path) -> Result<Installed> {
    let file = codex_home.join("config.toml");
    let (toml, previous) = apply_preset(&read_config(&file)?)?;
    let backup = backup(&file)?;

    let mut st = state::read(state_dir);
    // 重複安裝時保留第一次記下的原始值
    if !st.contains_key("codex") {
        st.insert(
            "codex".into(),
            json!({ "previousStatusLine": previous.status_line, "createdTui": previous.created_tui }),
        );
        state::write(state_dir, &st)?;
    }

    write_atomic(&file, &toml)?;
    Ok(Installed { file, backup, replaced_other: false })
}

pub fn uninstall(codex_home: &Path, state_dir: &Path) -> Result<Removal> {
    let file = codex_home.join("config.toml");
    let mut st = state::read(state_dir);
    let Some(saved) = st.remove("codex") else { return Ok(Removal::NotInstalled) };
    if !file.exists() {
        state::write(state_dir, &st)?;
        return Ok(Removal::NotInstalled);
    }
    let previous = Previous {
        status_line: saved.get("previousStatusLine").and_then(|v| v.as_str()).map(str::to_owned),
        created_tui: saved.get("createdTui").and_then(|v| v.as_bool()).unwrap_or(false),
    };
    let toml = revert_preset(&read_config(&file)?, &previous)?;
    write_atomic(&file, &toml)?;
    state::write(state_dir, &st)?;
    Ok(if previous.status_line.is_some() { Removal::Restored } else { Removal::Removed })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_missing_items_and_keeps_layout() {
        let original = "model = \"gpt-6.1\" # 主力模型\n\n[tui]\nscreen_reader_detection_done = true\nstatus_line = [\"model\", \"weekly-limit\"]\nstatus_line_use_colors = true\n\n[plugins.\"x\"]\nenabled = true\n";
        let (toml, previous) = apply_preset(original).unwrap();
        assert_eq!(previous.status_line.as_deref(), Some("[\"model\", \"weekly-limit\"]"));
        assert!(!previous.created_tui);
        assert!(toml.contains("model = \"gpt-6.1\" # 主力模型"));
        assert!(toml.contains("status_line = [\"model\", \"weekly-limit\", \"context-used\", \"five-hour-limit\"]"));
        assert!(toml.contains("[plugins.\"x\"]"));
        assert_eq!(revert_preset(&toml, &previous).unwrap(), original);
    }

    #[test]
    fn keeps_trailing_comment_through_roundtrip() {
        let original = "# 我的設定\n[tui]\nstatus_line = [\"model-with-reasoning\", \"weekly-limit\"] # 註解\n";
        let (toml, previous) = apply_preset(original).unwrap();
        assert_eq!(previous.status_line.as_deref(), Some("[\"model-with-reasoning\", \"weekly-limit\"]"));
        assert!(toml.contains("\"five-hour-limit\"] # 註解\n"));
        assert_eq!(revert_preset(&toml, &previous).unwrap(), original);
    }

    #[test]
    fn adds_default_items_when_missing() {
        let original = "[tui]\nstatus_line_use_colors = true\n";
        let (toml, previous) = apply_preset(original).unwrap();
        assert_eq!(previous, Previous { status_line: None, created_tui: false });
        assert!(toml.contains("\"five-hour-limit\""));
        assert_eq!(revert_preset(&toml, &previous).unwrap(), original);
    }

    #[test]
    fn creates_and_removes_tui_table() {
        let original = "model = \"gpt-6.1\"\n";
        let (toml, previous) = apply_preset(original).unwrap();
        assert!(previous.created_tui);
        assert!(toml.contains("[tui]"));
        assert_eq!(revert_preset(&toml, &previous).unwrap(), original);
    }

    #[test]
    fn already_complete_is_untouched() {
        let original = "[tui]\nstatus_line = [\"context-used\", \"five-hour-limit\", \"weekly-limit\"]\n";
        let (toml, _) = apply_preset(original).unwrap();
        assert_eq!(toml, original);
    }

    #[test]
    fn rejects_invalid_toml() {
        assert!(apply_preset("[tui\n").is_err());
        assert!(apply_preset("tui = 3\n").is_err());
    }

    #[test]
    fn install_and_uninstall_roundtrip_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join(".codex");
        let state_dir = dir.path().join("state");
        fs::create_dir_all(&home).unwrap();
        let original = "[tui]\nstatus_line = [\"model\"]\n";
        fs::write(home.join("config.toml"), original).unwrap();

        install(&home, &state_dir).unwrap();
        install(&home, &state_dir).unwrap();
        assert_eq!(uninstall(&home, &state_dir).unwrap(), Removal::Restored);
        assert_eq!(fs::read_to_string(home.join("config.toml")).unwrap(), original);
        assert_eq!(uninstall(&home, &state_dir).unwrap(), Removal::NotInstalled);
    }
}
